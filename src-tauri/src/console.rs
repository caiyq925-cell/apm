// 腾讯云控制台私有接口（Cookie 模式），BFF 模式对多服务通用：
//   POST https://console-hc.cloud.tencent.com/_api/{service}/{Action}
//        ?timeout=30000&t={ms}&uin={uin}&ownerUin={ownerUin}&csrfCode={code}
//   Body: {"cmd":"{Action}","serviceType":"{service}","data":{...OpenAPI 同构参数...},"regionId":4}
use crate::config::Config;
use serde_json::{json, Value};
use std::time::Duration;

pub async fn call_console(cfg: &Config, service: &str, version: &str, action: &str, mut data: Value) -> Result<Value, String> {
    let (uin, owner) = cfg.extract_ids_from_cookie();
    if uin.is_empty() || owner.is_empty() {
        return Err("无法确定 uin/ownerUin：请检查 Cookie 或在设置中手动填写".into());
    }
    if cfg.csrf_code.is_empty() {
        return Err("缺少 csrfCode：请从抓包请求的 URL 中复制 csrfCode 参数值填入设置".into());
    }

    if let Value::Object(m) = &mut data {
        // 各产品版本号不同（apm=2021-06-22、cdb=2017-03-20、monitor=2018-07-24、dbbrain=2019-10-16…），
        // 调用方未显式携带时才注入
        m.entry("Version".to_string()).or_insert(json!(version));
        m.entry("Language".to_string()).or_insert(json!("zh-CN"));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let url = format!(
        "https://console-hc.cloud.tencent.com/_api/{}/{}?timeout=30000&t={}&uin={}&ownerUin={}&csrfCode={}",
        service, action, now_ms, uin, owner, cfg.csrf_code
    );
    let body = json!({
        "cmd": action,
        "serviceType": service,
        "data": data,
        "regionId": cfg.region_id
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Origin", "https://console.cloud.tencent.com")
        .header("Referer", "https://console.cloud.tencent.com/monitor/apm/system/list")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36",
        )
        .header("Cookie", &cfg.cookie)
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|_| format!("响应非 JSON (HTTP {}): {}", status, snippet(&text)))?;
    normalize(v, status.as_u16())
}

fn snippet(s: &str) -> String {
    s.chars().take(300).collect()
}

/// 兼容多种返回包裹形态，统一剥出业务数据
fn normalize(v: Value, status: u16) -> Result<Value, String> {
    // 形态1: 直接是 TC3 Response
    if let Some(resp) = v.get("Response") {
        if let Some(err) = resp["Error"].as_object() {
            return Err(format!(
                "接口错误 {}: {}",
                err.get("Code").and_then(|x| x.as_str()).unwrap_or("Unknown"),
                err.get("Message").and_then(|x| x.as_str()).unwrap_or("")
            ));
        }
        return Ok(resp.clone());
    }
    // 形态2: {code, data:{Response}} / {code:0, data:{...}}
    let code = v.get("code").or_else(|| v.get("Code"));
    let ok_code = matches!(code, Some(Value::Number(n)) if n.as_i64().unwrap_or(-1) == 0)
        || matches!(code, Some(Value::String(s)) if s == "0" || s == "200");
    if ok_code {
        let data = v.get("data").or_else(|| v.get("Data")).cloned().unwrap_or(Value::Null);
        if let Some(resp) = data.get("Response") {
            if let Some(err) = resp["Error"].as_object() {
                return Err(format!(
                    "接口错误 {}: {}",
                    err.get("Code").and_then(|x| x.as_str()).unwrap_or("Unknown"),
                    err.get("Message").and_then(|x| x.as_str()).unwrap_or("")
                ));
            }
            return Ok(resp.clone());
        }
        if !data.is_null() {
            return Ok(data);
        }
    }
    Err(format!(
        "Cookie 接口返回异常 (HTTP {}): {}",
        status,
        snippet(&v.to_string())
    ))
}
