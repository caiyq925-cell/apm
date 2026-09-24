//! 旧网关 `/cgi/capi` 的容器 dashboard 退路取数。
//!
//! **主路径已经不需要这个文件**：`apm::call_capi` 用配置会话（csrf=bkn(skey)、配置 Cookie）
//! 直发旧网关，实测 `code=0` 且有数据（1216 的真根因是请求体被 `{"text":"..."}` 包装，
//! 网关解析不到顶层 `cmd` 就报"不合法的云 API 类型"，与令牌/TLS/会话无关）。
//!
//! 这里保留的 `fetch_dashboard` 是**退路**：万一脚本直发被网关拒（比如网关突然开始校验
//! 来源），就让隐藏的控制台页面**自己**发这条请求（不组装 HTTP，只塞一段 JS，网络层
//! 完全交给浏览器）。

use crate::config::{now_secs, Config};
use std::time::Duration;
use tauri::Manager;
use tokio::sync::Mutex;

/// 隐藏取数窗口必须**串行**：开头都会按前缀清理"残留窗口"，并发时会互相把对方的窗口
/// 关掉，表现是 CDP 连接被掐断（`Connection reset without closing handshake`）。
static WINDOW_LOCK: Mutex<()> = Mutex::const_new(());

const FETCH_LABEL_PREFIX: &str = "cloud-capi-fetch";

/// 退路：让隐藏的控制台页面**自己**用 `fetch` 发一次 dashboard 请求，把响应体取回来。
///
/// 为什么还需要它：主路径（`apm::call_capi` 配置直发）实测可用，但万一网关对脚本直发
/// 又加了别的校验（比如来源指纹），就靠它兜底——只把一段 JS 塞进页面执行，
/// 网络那一层完全交给浏览器，不再自己组装 HTTP。
///
/// 请求体同样要遵守"内层 JSON 直发 + JSON.stringify"的规矩（见 `build_fetch_js`），
/// 否则网关回 `请求包格式错误` 或 `code=1216`。
///
/// 返回：响应里的 `Data[]` 数组（已剥掉外层包裹）。
pub async fn fetch_dashboard(
    app: &tauri::AppHandle,
    cfg: &Config,
    service: &str,
    cmd: &str,
    data: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    if cfg.container_cluster.trim().is_empty() {
        return Err("未选择集群，无法打开取数页面".into());
    }
    let _guard = WINDOW_LOCK.lock().await;
    for (label, w) in app.webview_windows() {
        if label.starts_with(FETCH_LABEL_PREFIX) {
            let _ = w.destroy();
        }
    }
    // 刚销毁的窗口目标还挂在 /json/list 上一小会儿，立即连会连到"正在死"的目标（WebSocket reset）
    tokio::time::sleep(Duration::from_millis(400)).await;
    let label = format!("{}-{}", FETCH_LABEL_PREFIX, now_secs());
    let dep = cfg.selected_deployments.first().cloned().unwrap_or_default();
    let url = format!(
        "https://console.cloud.tencent.com/tke2/cluster/sub/detail/resource/deployment?rid={}&clusterId={}&resourceIns={}&np={}",
        cfg.region_id, cfg.container_cluster, dep, cfg.container_namespace
    )
    .parse()
    .map_err(|e| format!("控制台地址无效: {}", e))?;
    let _win = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("容器指标取数")
        .visible(false)
        .skip_taskbar(true)
        .additional_browser_args(crate::login::BROWSER_ARGS)
        .build()
        .map_err(|e| format!("打开取数窗口失败: {}", e))?;

    // 等页面自己发出 /cgi/capi 请求，用 CDP 抓它用的 csrfCode + x-lid/x-life + 浏览器 Cookie
    // （与预热同一个函数；拿不到会带出具体原因，比如"页面被送去登录页"）。
    let token = crate::login::read_capi_token(Duration::from_secs(35)).await?;
    let (uin, owner) = cfg.extract_ids_from_cookie();
    let js = build_fetch_js(
        &uin, &owner, service, cmd, data, cfg.region_id,
        &token.csrf, &token.x_lid, token.life_epoch_ms,
    );

    // 值已抓全，剩下的失败只会是页面上下文/网络抖动，重试几遍即可
    let mut last = String::new();
    let mut body = String::new();
    for _ in 0..3 {
        match crate::login::eval_in_page(&js, Duration::from_secs(30)).await {
            Ok(t) if t == "NO_FETCH" => last = "页面里 fetch 不可用".into(),
            Ok(t) => {
                body = t;
                break;
            }
            Err(e) => last = e,
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }
    if body.is_empty() {
        return Err(format!("退路取数失败：{}", last));
    }

    let v: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| format!("退路取数响应非 JSON: {}", body.chars().take(200).collect::<String>()))?;
    let code = v["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = v["msg"].as_str().unwrap_or("");
        // code=-1（或响应里根本没有 code 字段）时把原始响应带出来，方便看出网关到底回了什么
        let extra = if msg.is_empty() {
            format!(" 原始响应: {}", body.chars().take(300).collect::<String>())
        } else {
            String::new()
        };
        return Err(format!(
            "旧网关接口错误 code={}: {}（在页面里发也一样，说明不是「谁发」的问题）{}",
            code, msg, extra
        ));
    }
    let d = &v["data"];
    if d["data"]["Response"].is_object() {
        Ok(d["data"]["Response"]["Data"].clone())
    } else if d["Response"].is_object() {
        Ok(d["Response"]["Data"].clone())
    } else {
        Ok(d["Data"].clone())
    }
}

/// 生成"在页面里自己发请求"的 JS。
///
/// csrfCode / `x-lid` / `x-life` 基准**全部来自 CDP 抓到的页面自己那份请求**（`read_capi_token`，
/// 不需要在页面里翻 performance 记录——那里只有 URL，拿不到头）。`x-life` 在 JS 里按
/// `Date.now() - life_epoch` 现场生成，保证是"没用过的"新鲜值（网关认"用过即废"）。
///
/// **请求体 = 内层 JSON 直接发**（`{"cmd":..,"serviceType":..,"data":{..},"regionId":4}`），
/// 绝不能包 `{"text":"...","mime":..,"encoding":..}`——抓包逐条核过（content-length 与
/// body 长度精确一致），`mime`/`encoding` 是 Reqable 的元数据字段、不是协议的一部分。
/// 包了它网关就解析不到顶层 `cmd`，回 `code=1216 不合法的云 API 类型`。
///
/// **同时必须 `JSON.stringify` 成字符串**：`fetch` 的 `body` 收到普通对象不会序列化，
/// 而是变成 `"[object Object]"` 发出去——网关收到这 16 个字节就回
/// `{"message":"请求包格式错误或大小超出限制"}`（真机日志实锤）。
fn build_fetch_js(
    uin: &str,
    owner: &str,
    service: &str,
    cmd: &str,
    data: &serde_json::Value,
    region_id: i64,
    csrf: &str,
    x_lid: &str,
    life_epoch_ms: i64,
) -> String {
    let body = serde_json::json!({ "cmd": cmd, "serviceType": service, "data": data, "regionId": region_id });
    // serde_json 生成的字符串字面量与 JS 兼容，可直接嵌进表达式
    let body_lit = serde_json::to_string(&body).unwrap_or_else(|_| "\"\"".to_string());
    // x-lid/x-life 缺一不可（抓包核过，缺了被网关拒）；x_lid 值只有字母数字和短横线，但照例转义
    let lid_escaped = serde_json::to_string(x_lid.trim()).unwrap_or_else(|_| "\"\"".to_string());
    let lid_hdr = if x_lid.trim().is_empty() {
        String::new()
    } else if life_epoch_ms > 0 {
        format!(r#", "x-lid": {}, "x-life": String(Date.now() - {})"#, lid_escaped, life_epoch_ms)
    } else {
        format!(r#", "x-lid": {}"#, lid_escaped)
    };
    format!(
        r#"(async () => {{
  if (typeof fetch !== "function") return "NO_FETCH";
  const url = "https://console.cloud.tencent.com/cgi/capi?cmd={cmd}&action=delegate&serviceType={service}"
    + "&secure=1&version=3&dictId=2006&sts=1&t=" + Date.now()
    + "&uin={uin}&ownerUin={owner}&csrfCode={csrf}";
  const r = await fetch(url, {{ method: "POST", credentials: "include",
    headers: {{ "Content-Type": "application/json", "Accept": "application/json, text/javascript, */*; q=0.01", "X-Requested-With": "XMLHttpRequest"{lid_hdr} }},
    body: JSON.stringify({body}) }});
  return await r.text();
}})()"#,
        cmd = cmd,
        service = service,
        uin = uin,
        owner = owner,
        csrf = csrf,
        body = body_lit,
        lid_hdr = lid_hdr
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_js_embeds_captured_credentials_and_payload() {
        let data = serde_json::json!({ "Query": [{ "MetricName": "K8sPodRateCpuCoreUsedLimit" }] });
        let js = build_fetch_js(
            "765378326",
            "100012781415",
            "monitor",
            "DescribeDashboardMetricData",
            &data,
            4,
            "1286230423",
            "sJxn-Y7DfV",
            1790078075676,
        );
        // csrfCode / x-lid / x-life 基准都是 CDP 抓来的（页面自己那份请求），直接嵌进 URL/头
        assert!(js.contains("csrfCode=1286230423"), "{}", js);
        assert!(js.contains("\"x-lid\": \"sJxn-Y7DfV\""), "{}", js);
        // x-life 必须现场生成（Date.now() - life_epoch），不能复用抓到的旧值
        assert!(js.contains("\"x-life\": String(Date.now() - 1790078075676)"), "{}", js);
        assert!(js.contains("/cgi/capi"));
        // 旧版"从 performance 记录里现翻"已不需要：值全在捕获里，页面自己发的请求就是来源
        assert!(!js.contains("getEntriesByType"), "{}", js);
        assert!(js.contains("NO_FETCH"));
        // 账号与载荷要进去
        assert!(js.contains("uin=765378326"));
        assert!(js.contains("ownerUin=100012781415"));
        assert!(js.contains("K8sPodRateCpuCoreUsedLimit"));
        assert!(js.contains("regionId"));
        // URL 里绝不能带 json=1（加了必被网关判成另一种云 API 类型）
        assert!(!js.contains("json=1"), "{}", js);
        // 必须在页面上下文里带凭证发（用浏览器自己的 Cookie）
        assert!(js.contains("credentials: \"include\""));
        // 请求体必须 JSON.stringify：裸对象会被 fetch 变成 "[object Object]"，
        // 网关收到就回 "请求包格式错误或大小超出限制"（真机日志实锤，不要删掉这行断言）
        assert!(js.contains("body: JSON.stringify("), "{}", js);
        // 请求体 = 内层 JSON **直发**，绝不能包 {"text":"...","mime":..}：
        // 抓包 content-length 实锤（71 条全吻合），包了网关解析不到顶层 cmd 回 1216
        assert!(js.contains("\"cmd\":\"DescribeDashboardMetricData\""), "{}", js);
        assert!(!js.contains("mime"), "{}", js);
        assert!(!js.contains("encoding"), "{}", js);
    }

    #[test]
    fn fetch_js_omits_x_headers_when_not_captured() {
        let data = serde_json::json!({});
        let js = build_fetch_js("u", "o", "monitor", "DescribeDashboardMetricData", &data, 4, "csrf", "", 0);
        assert!(!js.contains("x-lid"), "{}", js);
        assert!(!js.contains("x-life"), "{}", js);
        assert!(js.contains("csrfCode=csrf"), "{}", js);
    }
}
