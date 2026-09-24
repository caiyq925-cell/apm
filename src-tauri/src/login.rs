// 内嵌登录：打开腾讯云官方登录页（微信扫码），登录后经 CDP 取回会话信息写入配置。
// 全程在腾讯官方页面完成扫码，工具不接触账号密码。
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

/// CDP 用的 WebSocket 连接类型（`connect_async` 的返回类型），给几个辅助函数复用
type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

// 调试端口必须与 tauri.conf.json 中主窗口的 additionalBrowserArgs 一致：
// Windows 上 WebView2 环境按 data directory 共享，端口只能在创建环境时（主窗口）打开，
// 登录窗口单独传 additional_browser_args 会被静默忽略。
pub const CDP_PORT: u16 = 9223;

/// 必须与 tauri.conf.json 主窗口的 additionalBrowserArgs **完全一致**。
/// 同一 data directory 下的多个 webview 若该参数不同，WebView2 建不出第二个 webview
/// （见 tauri-utils `WindowConfig::additional_browser_args` 文档：不同的值必须配不同 data directory），
/// 表现就是"日志说窗口已打开、界面上却没有窗口"。
pub const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,LocalNetworkAccessChecks,PrivateNetworkAccessSendPreflights,PrivateNetworkAccessRespectPreflightResults,BlockInsecurePrivateNetworkRequests --remote-debugging-port=9223";
pub const LOGIN_URL: &str = "https://console.cloud.tencent.com/monitor/apm/system/list";

pub struct LoginResult {
    pub cookie: String,
    pub uin: String,
    pub owner_uin: String,
    pub csrf_code: String,
    /// 本次是否由工具自动点击了「微信快捷登录」（供调用方打日志）
    pub auto_quick_clicked: bool,
}

fn extract(url: &str, key: &str) -> Option<String> {
    let idx = url.find(key)?;
    let rest = &url[idx + key.len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .unwrap_or(rest.len());
    if end == 0 { None } else { Some(rest[..end].to_string()) }
}

/// 同 extract，但把占位值 `0` 视为"没提供"：控制台请求里常见 `ownerUin=0`，
/// 真值要到 Cookie 里取（ownerUin=O100012781415G），直接采用 0 会让所有接口报错。
fn extract_id(url: &str, key: &str) -> String {
    match extract(url, key) {
        Some(v) if v != "0" => v,
        _ => String::new(),
    }
}

/// 等待登录窗口的调试目标出现，返回 WS 地址
async fn find_target() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    // 端口正常时第一次就能命中；连不上也不要死等——TCP 可能长时间停在 SYN_SENT，
    // 按次数循环会把 3 秒超时放大成好几分钟
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if let Ok(resp) = client.get(format!("http://127.0.0.1:{}/json/list", CDP_PORT)).send().await {
            if let Ok(list) = resp.json::<Vec<Value>>().await {
                for t in list {
                    let ty = t.get("type").and_then(|x| x.as_str()).unwrap_or("");
                    let url = t.get("url").and_then(|x| x.as_str()).unwrap_or("");
                    if ty == "page" && url.contains("tencent.com") {
                        if let Some(ws) = t.get("webSocketDebuggerUrl").and_then(|x| x.as_str()) {
                            return Ok(ws.to_string());
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(format!(
        "连接不上调试端口 127.0.0.1:{}（请确认 tauri.conf.json 主窗口的 additionalBrowserArgs 带 --remote-debugging-port={}）",
        CDP_PORT, CDP_PORT
    ))
}

/// 退路（`capi::fetch_dashboard`）要从页面自己发的 `/cgi/capi` 请求里拿的三样东西：
/// 它认的 csrfCode、`x-lid`，以及 x-life 的生成基准 `life_epoch_ms`。
///
/// `x-lid` / `x-life` 是控制台页面给自己所有 `/cgi/capi` 请求带的两个头（抓包核过）：
/// `x-lid` 会话内恒定，`x-life` **每个请求都不同**且满足 `x-life = t_ms - life_epoch_ms`
/// （life_epoch_ms 是会话常数）。发请求时 `x-life` 用 `now_ms - life_epoch_ms` 现场生成。
///
/// 注意：**主路径（脚本直发）已经不需要这些**——`csrf = bkn(skey)` 现算、Cookie 用配置的，
/// 实测 code=0（1216 真根因是请求体包装，见 `apm::call_capi` 与 `login::bkn`）。
#[derive(Clone)]
pub struct CapiToken {
    pub csrf: String,
    pub x_lid: String,
    /// 会话常数：`页面请求的 t_ms - 该请求的 x-life`。0 表示没抓到（发请求时跳过 x-life 头）。
    pub life_epoch_ms: i64,
}

/// 腾讯控制台 csrfCode 的生成算法：`bkn(skey)`。
///
/// 抓包实锤（2026-09-24）：`{"text":"..."}` 包装才是 1216 的根因，csrf 本身一直是
/// 这个哈希——抓包里所有请求的 `csrfCode` == `bkn(cookie 里的 skey)`（`/_api` 和
/// `/cgi/capi` 是同一个值）。所以**根本不需要从页面抓 csrf**，skey 有效期内它就是
/// 确定的，会话轮换后算出新值即可。
///
/// 递推 `h = 33*h + ord(ch)`（线性、系数为整数），最终只取低 31 位，所以低 31 位
/// 只依赖上一步的低 31 位——用 u32 环绕算与全精度结果等价（Python 对照验证过）。
pub fn bkn(skey: &str) -> i64 {
    let mut hash: u32 = 5381;
    for ch in skey.chars() {
        hash = hash.wrapping_mul(33).wrapping_add(ch as u32);
    }
    (hash & 0x7fff_ffff) as i64
}

/// 从一条 `Network.requestWillBeSent` 事件里取旧网关请求带的头信息。
///
/// 返回 `(x_lid, life_epoch_ms)`：`x_lid` 是会话内恒定的那一个头；`life_epoch_ms` 是会话常数
/// （`该请求的 t_ms - 该请求的 x-life`），用它就能现场生成新鲜的 `x-life`。头名大小写不定，
/// 请求头与大小写都要容错；抓不到任一项就回空/0，调用方按"缺项"处理。
fn capi_request_meta(v: &Value) -> (String, i64) {
    let url = v["params"]["request"]["url"].as_str().unwrap_or("");
    let headers = v["params"]["request"]["headers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let hdr = |key: &str| -> String {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, val)| val.as_str())
            .unwrap_or("")
            .to_string()
    };
    let x_lid = hdr("x-lid");
    let t = extract(url, "t=").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let life_epoch = hdr("x-life")
        .parse::<i64>()
        .ok()
        .filter(|_| t > 0)
        .map(|life| t - life)
        .filter(|epoch| *epoch > 0)
        .unwrap_or(0);
    (x_lid, life_epoch)
}

/// 这个页面是不是被控制台送到了登录页。
///
/// 判断依据：URL 里出现 login / sso / auth 这类片段，且**不是**我们的目标控制台页
/// （`/tke2/...`）。未登录时控制台会把页面送去登录，那种情况下一个 `/cgi/capi`
/// 请求都不会发出——干等 35 秒再报"没取到令牌"既慢、又没告诉用户该做什么。
fn is_login_url(href: &str) -> bool {
    let h = href.to_ascii_lowercase();
    if h.contains("/tke2/") {
        return false;
    }
    h.contains("/login") || h.contains("login.") || h.contains("/sso") || h.contains("auth.")
}

/// 问一下页面当前在哪（`location.href`）。只在"没有事件"的空闲间隙调用，
/// 这样不会把正在路上的 `Network.requestWillBeSent` 事件吃掉。
async fn eval_href(ws: &mut Ws, next_id: &mut u64) -> Option<String> {
    let id = *next_id;
    *next_id += 1;
    let _ = send_cmd(
        ws,
        id,
        "Runtime.evaluate",
        json!({ "expression": "location.href", "returnByValue": true }),
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
        let txt = match msg {
            Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
            Ok(Some(Ok(_))) => continue,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("id").and_then(|x| x.as_u64()) == Some(id) {
            return v["result"]["result"]["value"].as_str().map(|s| s.to_string());
        }
    }
    None
}

/// 读隐藏控制台页面自己发出的 `/cgi/capi` 请求里用的 csrfCode，顺带取回浏览器当前的 Cookie。
///
/// 旧网关只认"控制台页面自己发请求时用的那个 csrfCode"（与会话绑定、寿命约十分钟），
/// 官方 /_api 那套 csrfCode 在这个网关一律返回 `code=1216 不合法的云 API 类型`。
///
/// 顺带抓 `x-lid` / `x-life` 两个头（旧网关的要求，见 [`CapiToken`]）。传输层抖动
/// （典型如上一扇隐藏窗刚被销毁、目标还挂在 `/json/list` 里）表现为 WebSocket reset，
/// 这种时候重连一次往往就好——不要就地失败，否则用户只看到"调试通道错误"。
pub async fn read_capi_token(timeout: Duration) -> Result<CapiToken, String> {
    match read_capi_token_once(timeout).await {
        Err(e) if e.starts_with("调试通道错误") => read_capi_token_once(timeout).await,
        r => r,
    }
}

async fn read_capi_token_once(timeout: Duration) -> Result<CapiToken, String> {
    let ws_url = find_target().await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("连接调试端口失败: {}", e))?;
    let mut next_id = 1u64;
    let _ = send_cmd(&mut ws, next_id, "Network.enable", json!({})).await;
    next_id += 1;

    let deadline = tokio::time::Instant::now() + timeout;
    let mut csrf = String::new();
    let mut x_lid = String::new();
    let mut life_epoch_ms: i64 = 0;
    let mut probes = 0u32;
    while tokio::time::Instant::now() <= deadline {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next()).await;
        let txt = match msg {
            Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(format!("调试通道错误: {}", e)),
            Ok(None) => return Err("预热窗口已关闭".into()),
            Err(_) => {
                // 空闲间隙：看一眼页面在哪。未登录时快速失败并**标 cred** ——
                // 那是真的"需要重新登录"，前端会据此把扫码窗口拉起来。
                if probes < 5 {
                    probes += 1;
                    if let Some(href) = eval_href(&mut ws, &mut next_id).await {
                        if is_login_url(&href) {
                            return Err(crate::cred::cred(format!(
                                "登录窗口里没有登录态（控制台页面被送到了 {}）：请在「设置」里点「扫码登录」完成扫码",
                                href.chars().take(60).collect::<String>()
                            )));
                        }
                    }
                }
                continue;
            }
        };
        let v: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("method").and_then(|m| m.as_str()) != Some("Network.requestWillBeSent") {
            continue;
        }
        let url = match v["params"]["request"]["url"].as_str() {
            Some(u) => u,
            None => continue,
        };
        // 旧网关请求的 URL 一定带 csrfCode；x-lid/x-life 在请求头里（头名大小写不定）。
        // 三者齐了就收工——页面后续请求里这些值不变，够用了。
        if !(url.contains("/cgi/capi") && url.contains("csrfCode=")) {
            continue;
        }
        if csrf.is_empty() {
            if let Some(tok) = extract(url, "csrfCode=") {
                csrf = tok;
            }
        }
        if x_lid.is_empty() || life_epoch_ms == 0 {
            let (lid, epoch) = capi_request_meta(&v);
            if x_lid.is_empty() {
                x_lid = lid;
            }
            if life_epoch_ms == 0 {
                life_epoch_ms = epoch;
            }
        }
        if !csrf.is_empty() && !x_lid.is_empty() && life_epoch_ms > 0 {
            break;
        }
    }
    if csrf.is_empty() {
        // 不标 cred：这是"取数页面没把 dashboard 请求发出来"，属于令牌层面的问题，
        // 不是整机会话失效，不该触发重新登录。
        // 最常见原因是**浏览器会话没有登录态**（控制台页面被重定向到登录页，
        // 自然不会有 capi 请求）—— 比如 WebView2 数据目录被清过之后就会这样。
        return Err(
            "没能从控制台页面取到旧网关令牌（页面没有发出 dashboard 请求：可能是工作负载页打不开，\
             也可能登录窗口里没有登录态 —— 展开「设置」点一次「扫码登录」即可）"
                .to_string(),
        );
    }

    Ok(CapiToken { csrf, x_lid, life_epoch_ms })
}

async fn send_cmd<S>(ws: &mut S, id: u64, method: &str, params: Value) -> Result<(), String>
where
    S: SinkExt<Message> + Unpin,
{
    let msg = json!({"id": id, "method": method, "params": params});
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .map_err(|_| "调试通道发送失败".to_string())
}

fn build_cookie(v: &Value) -> String {
    use std::collections::HashMap;
    // 同名 Cookie 可能出现在多个域上（.tencent.com / .cloud.tencent.com）。扁平化成一个
    // Cookie 头时：跳过空值，并优先取控制台域上的那份——否则空值会盖掉真值
    // （实测过：`uin=` 空值把 `uin=o765378326` 顶掉，保存后所有接口都报错）。
    let mut order: Vec<String> = Vec::new();
    let mut best: HashMap<String, (u8, String)> = HashMap::new();
    if let Some(cookies) = v["result"]["cookies"].as_array() {
        for c in cookies {
            let domain = c.get("domain").and_then(|d| d.as_str()).unwrap_or("");
            if !domain.ends_with("tencent.com") {
                continue;
            }
            let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let value = c.get("value").and_then(|x| x.as_str()).unwrap_or("");
            if name.is_empty() || value.is_empty() {
                continue;
            }
            let score: u8 = if domain.contains("cloud.tencent.com") { 2 } else { 1 };
            let keep_old = matches!(best.get(name), Some((old, _)) if *old >= score);
            if keep_old {
                continue;
            }
            if !best.contains_key(name) {
                order.push(name.to_string());
            }
            best.insert(name.to_string(), (score, value.to_string()));
        }
    }
    order
        .into_iter()
        .filter_map(|n| best.get(&n).map(|(_, v)| format!("{}={}", n, v)))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Cookie 串里是否有某个键（按 `;` 分段、前缀大小写不敏感匹配，避免 "uin" 命中 "ownerUin"）
fn cookie_has(c: &str, key: &str) -> bool {
    // 必须是非空值：`uin=` 这种空值不算"有 uin"
    c.split(';').any(|p| {
        let p = p.trim();
        p.len() > key.len() + 1
            && p[..key.len()].eq_ignore_ascii_case(key)
            && p.as_bytes()[key.len()] == b'='
            && !p[key.len() + 1..].trim().is_empty()
    })
}

/// 控制台会话 Cookie 一定带 uin；只有登录类 Cookie 说明页面还没建立控制台会话
pub fn cookie_has_uin(c: &str) -> bool {
    cookie_has(c, "uin")
}

/// 控制台会话写全了的标志：uin 是控制台会话的前提，ownerUin / refreshSession 是页面
/// 自己发过请求后才落盘的。三者缺一都说明抓到的还是"半套" Cookie，
/// 存进去服务端会判 `code=9`，所以宁可判失败也不要覆盖掉能用的会话。
fn cookie_is_complete(c: &str) -> bool {
    cookie_has(c, "uin") && (cookie_has(c, "owneruin") || cookie_has(c, "refreshsession"))
}

/// 诊断用：描述 Cookie 的"完整度"。只打"有/无"，**不打任何值**。
///
/// 用来定位"扫码偶发拿到半套 Cookie"：对比成功那几次和失败那次的这一行，
/// 就能看出到底是缺哪个 Cookie 导致服务端判 `code=9`。
pub fn describe_cookie(c: &str) -> String {
    let segs = c.split(';').filter(|p| p.contains('=')).count();
    let has = |k: &str| if cookie_has(c, k) { "有" } else { "无" };
    format!(
        "段数 {}，uin={}，ownerUin={}，refreshSession={}，saas_synced_session={}",
        segs,
        has("uin"),
        has("owneruin"),
        has("refreshsession"),
        has("saas_synced_session")
    )
}

/// 微信「快捷登录」探测：腾讯云登录页在已绑定微信时会显示「快捷登录」按钮，点一下即登录、免扫码。
/// 识别限定登录态页面（URL 含 login/auth/sso 且非 /tke2/），正文出现「快捷登录/一键登录/免扫码」，
/// 取文字命中的**最小**元素（最贴近按钮本体），再向上找真正可点击的祖先（button/a/onclick/pointer），
/// 返回按钮中心的视口坐标（供 `Input.dispatchMouseEvent` 兜底）。找不到返回 'not-login'/'not-found'/'hidden'。
const QUICK_LOGIN_PROBE: &str = r#"(function(){
  if (!/login|auth|sso/i.test(location.href) || location.href.includes('/tke2/')) return 'not-login';
  const kw = /快捷登录|一键登录|免扫码登录/;
  let best = null, bestLen = 1e9;
  for (const el of document.querySelectorAll('button, a, [role="button"], div, span, em, i, li, p')) {
    const t = (el.innerText || '').replace(/\s+/g, '');
    if (!t || !kw.test(t)) continue;
    if (t.length < bestLen) { bestLen = t.length; best = el; }
  }
  if (!best) return 'not-found';
  let target = best;
  while (target && target !== document.body) {
    if (target.tagName === 'BUTTON' || target.tagName === 'A' || target.onclick
        || getComputedStyle(target).cursor === 'pointer') break;
    target = target.parentElement;
  }
  if (!target || target === document.body) target = best;
  const r = target.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0) return 'hidden';
  return JSON.stringify({ status: 'found', x: r.x + r.width / 2, y: r.y + r.height / 2,
                          text: (target.innerText || '').replace(/\s+/g, '').slice(0, 24) });
})()"#;

/// 微信「快捷登录」点击（原生 click）：与探测同一套找法，找到就点。
/// 返回 'gone'（按钮没了）/ 'hidden'（不可见）/ 'clicked'（已点）。
const QUICK_LOGIN_CLICK: &str = r#"(function(){
  const kw = /快捷登录|一键登录|免扫码登录/;
  let best = null, bestLen = 1e9;
  for (const el of document.querySelectorAll('button, a, [role="button"], div, span, em, i, li, p')) {
    const t = (el.innerText || '').replace(/\s+/g, '');
    if (!t || !kw.test(t)) continue;
    if (t.length < bestLen) { bestLen = t.length; best = el; }
  }
  if (!best) return 'gone';
  let target = best;
  while (target && target !== document.body) {
    if (target.tagName === 'BUTTON' || target.tagName === 'A' || target.onclick
        || getComputedStyle(target).cursor === 'pointer') break;
    target = target.parentElement;
  }
  if (!target || target === document.body) target = best;
  const r = target.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0) return 'hidden';
  target.click();
  return 'clicked';
})()"#;

/// 微信 PC 的本地端口（14013 等）只接受 TLS 1.2，WebView2 握手用 TLS 1.3 会被直接掐断
/// （`net::ERR_CONNECTION_CLOSED`），登录页就探测不到微信、一直停在二维码。
/// 这里拦住 iframe 发往 `localhost.weixin.qq.com` 的请求，用系统 curl 的 TLS 1.2 代发再回填。
fn spawn_wechat_fast_login_bridge() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(2)).build() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut live: std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>> =
            std::collections::HashMap::new();
        loop {
            live.retain(|_, done| !done.load(std::sync::atomic::Ordering::Relaxed));
            if let Ok(resp) = client.get(format!("http://127.0.0.1:{}/json/list", CDP_PORT)).send().await {
                if let Ok(list) = resp.json::<Vec<Value>>().await {
                    for t in list {
                        let url = t.get("url").and_then(|x| x.as_str()).unwrap_or("");
                        let ws = t.get("webSocketDebuggerUrl").and_then(|x| x.as_str()).unwrap_or("");
                        if url.contains("open.weixin.qq.com") && !ws.is_empty() && !live.contains_key(ws) {
                            let ws = ws.to_string();
                            let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            live.insert(ws.clone(), done.clone());
                            tokio::spawn(async move {
                                let _ = bridge_weixin_iframe(ws).await;
                                done.store(true, std::sync::atomic::Ordering::Relaxed);
                            });
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    })
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn bridge_weixin_iframe(ws_url: String) -> Result<(), String> {
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| e.to_string())?;
    let mut next_id = 1u64;
    send_cmd(
        &mut ws,
        next_id,
        "Fetch.enable",
        json!({
            "patterns": [{
                "urlPattern": "*localhost.weixin.qq.com*",
                "requestStage": "Request"
            }]
        }),
    )
    .await?;
    next_id += 1;
    // 页面可能已经用 TLS 1.3 探测失败并退回二维码，拦下之后刷新一次让它重探。
    let _ = send_cmd(&mut ws, next_id, "Page.reload", json!({})).await;
    next_id += 1;

    while let Some(msg) = ws.next().await {
        let txt = match msg {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(_) => continue,
            Err(_) => break,
        };
        let v: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("method").and_then(|m| m.as_str()) != Some("Fetch.requestPaused") {
            continue;
        }
        let request_id = v["params"]["requestId"].as_str().unwrap_or("").to_string();
        let req_url = v["params"]["request"]["url"].as_str().unwrap_or("").to_string();
        let req_method = v["params"]["request"]["method"].as_str().unwrap_or("GET").to_string();
        let post = v["params"]["request"]["postData"].as_str().unwrap_or("").to_string();
        if request_id.is_empty() {
            continue;
        }
        let fulfilled = if req_method.eq_ignore_ascii_case("OPTIONS") {
            Some((200u16, Vec::new()))
        } else if req_url.contains("/api/") {
            wechat_local_exchange(&req_method, &req_url, &post).await
        } else {
            None
        };
        if let Some((code, body)) = fulfilled {
            let _ = send_cmd(
                &mut ws,
                next_id,
                "Fetch.fulfillRequest",
                json!({
                    "requestId": request_id,
                    "responseCode": code,
                    "responseHeaders": [
                        {"name": "Access-Control-Allow-Origin", "value": "https://open.weixin.qq.com"},
                        {"name": "Access-Control-Allow-Methods", "value": "POST, GET, OPTIONS"},
                        {"name": "Access-Control-Allow-Headers", "value": "Content-Type"},
                        {"name": "Access-Control-Allow-Private-Network", "value": "true"},
                        {"name": "Content-Type", "value": "application/json"}
                    ],
                    "body": b64(&body)
                }),
            )
            .await;
        } else {
            let _ = send_cmd(
                &mut ws,
                next_id,
                "Fetch.continueRequest",
                json!({"requestId": request_id}),
            )
            .await;
        }
        next_id += 1;
    }
    Ok(())
}

async fn wechat_local_exchange(method: &str, url: &str, body: &str) -> Option<(u16, Vec<u8>)> {
    let method = method.to_string();
    let url = url.to_string();
    let body = body.to_string();
    tokio::task::spawn_blocking(move || {
        let mut child = std::process::Command::new("curl.exe")
            .args([
                "-sk",
                "--tlsv1.2",
                "--tls-max",
                "1.2",
                "-m",
                "8",
                "-X",
                &method,
                &url,
                "-H",
                "Content-Type: application/json",
                "-H",
                "Origin: https://open.weixin.qq.com",
                "-w",
                "\n%{http_code}",
                "--data-binary",
                "@-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        {
            use std::io::Write;
            let stdin = child.stdin.as_mut()?;
            stdin.write_all(body.as_bytes()).ok()?;
        }
        let out = child.wait_with_output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let (payload, code) = match text.rfind('\n') {
            Some(i) => (&text[..i], text[i + 1..].trim().parse::<u16>().unwrap_or(200)),
            None => (text.as_ref(), 200u16),
        };
        Some((code, payload.as_bytes().to_vec()))
    })
    .await
    .ok()
    .flatten()
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if i + 1 < data.len() {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        out.push('=');
    }
    out
}

/// 连接登录窗口，等待用户扫码登录，返回会话信息
/// timeout：等待上限。查询中的自动刷新用短超时（别让统计卡太久）；
/// 用户手动点「扫码登录」用长超时（找手机、扫码要时间），超时前关掉窗口会立刻返回。
/// auto_quick_login：登录页出现「微信快捷登录」按钮时自动点击（免扫码）。只在手动可见窗口开。
pub async fn wait_for_login(timeout: Duration, auto_quick_login: bool) -> Result<LoginResult, String> {
    let _bridge = AbortOnDrop(spawn_wechat_fast_login_bridge());
    let ws_url = find_target().await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("连接调试端口失败: {}", e))?;

    let mut next_id = 1u64;
    let _ = send_cmd(&mut ws, next_id, "Network.enable", json!({})).await;
    next_id += 1;
    // 不要再 Page.navigate：
    // 窗口本来就是用 LOGIN_URL 建的，页面正在自己加载；此时再导航一次会打断控制台页面的初始化，
    // 结果抓到的是一套"半初始化"的 Cookie（缺 saas_synced_session / web_uid 等），
    // 存进去后所有控制台接口都会报 code=9 登录态验证失败。
    // 未登录时控制台自己会跳登录页，扫码后自动跳回来，无需我们插手。

    let mut csrf_code = String::new();
    let mut uin = String::new();
    let mut owner_uin = String::new();
    let mut cookie = String::new();
    let mut cookie_req_id: Option<u64> = None;
    let mut last_cookie_poll = tokio::time::Instant::now();
    // 会话就绪后用于"等 Cookie 写全并稳定再取最后一次快照"
    let mut settle_start: Option<tokio::time::Instant> = None;
    let mut final_req: Option<u64> = None;
    let mut last_snapshot = String::new();
    let mut stable_since: Option<tokio::time::Instant> = None;
    // —— 微信「快捷登录」自动点击状态机（仅手动可见窗口、开关打开时）——
    let mut quick_native_done = false; // 已用原生 click 点过一次
    let mut quick_mouse_done = false; // 已用真实鼠标事件兜底点过一次
    let mut quick_clicked_at: Option<tokio::time::Instant> = None;
    let mut quick_coords: Option<(i64, i64)> = None;
    let mut quick_probe_id: Option<u64> = None;
    let mut last_quick_probe = tokio::time::Instant::now();

    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err(if cookie_has_uin(&cookie) {
                "等待登录超时：请在登录窗口中完成扫码登录".into()
            } else {
                "等待登录超时：未检测到登录态（Cookie 里没有 uin），请在登录窗口中完成扫码登录".to_string()
            });
        }
        // 每 1.5s 取一次 Cookie 快照：登录前用来发现 uin，登录后用来判断是否写全 / 是否稳定
        if final_req.is_none() && last_cookie_poll.elapsed() > Duration::from_millis(1500) {
            let id = next_id;
            next_id += 1;
            cookie_req_id = Some(id);
            let _ = send_cmd(&mut ws, id, "Network.getAllCookies", json!({})).await;
            last_cookie_poll = tokio::time::Instant::now();
        }
        // 会话就绪（csrfCode + uin）后不要立刻收工：控制台是分批写 Cookie 的
        // （ownerUin / appid / saas_synced_session …），早取只会拿到半套，
        // 存进配置后所有控制台接口都会报"登录态验证失败"。这里等它稳定下来再取。
        if final_req.is_none() && !csrf_code.is_empty() && cookie_has_uin(&cookie) {
            let start = *settle_start.get_or_insert_with(tokio::time::Instant::now);
            let stable = stable_since.map_or(false, |t| t.elapsed() > Duration::from_millis(1500));
            let give_up = start.elapsed() > Duration::from_secs(25);
            if cookie_is_complete(&cookie) && (stable || give_up) {
                let id = next_id;
                next_id += 1;
                final_req = Some(id);
                let _ = send_cmd(&mut ws, id, "Network.getAllCookies", json!({})).await;
            }
        }
        // 自动快捷登录：登录页上出现「快捷登录」就点一下，免扫码。
        // 原生 click 若被页面忽略，5 秒后还没出会话就用真实鼠标事件在按钮中心补一刀。
        if auto_quick_login && !quick_mouse_done && last_quick_probe.elapsed() > Duration::from_millis(1500) {
            last_quick_probe = tokio::time::Instant::now();
            let id = next_id;
            next_id += 1;
            quick_probe_id = Some(id);
            let _ = send_cmd(&mut ws, id, "Runtime.evaluate", json!({
                "expression": QUICK_LOGIN_PROBE, "returnByValue": true
            })).await;
        }
        if auto_quick_login
            && quick_native_done
            && !quick_mouse_done
            && quick_clicked_at.map_or(false, |t| t.elapsed() > Duration::from_secs(5))
            && !cookie_has_uin(&cookie)
            && quick_coords.is_some()
        {
            let (x, y) = quick_coords.unwrap();
            quick_mouse_done = true;
            let id = next_id;
            next_id += 1;
            let _ = send_cmd(&mut ws, id, "Input.dispatchMouseEvent", json!({
                "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1
            })).await;
            let id = next_id;
            next_id += 1;
            let _ = send_cmd(&mut ws, id, "Input.dispatchMouseEvent", json!({
                "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1
            })).await;
        }

        let msg = tokio::time::timeout(Duration::from_secs(3), ws.next()).await;
        let txt = match msg {
            Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(format!("调试通道错误: {}", e)),
            Ok(None) => return Err("登录窗口已关闭".into()),
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if v.get("method").and_then(|m| m.as_str()) == Some("Network.requestWillBeSent") {
            if let Some(url) = v["params"]["request"]["url"].as_str() {
                if url.contains("console.cloud.tencent.com") {
                    // uin/ownerUin 不一定出现在第一条请求里，后续请求里持续补全（已有值不覆盖）
                    if uin.is_empty() {
                        uin = extract_id(url, "uin=");
                    }
                    if owner_uin.is_empty() {
                        owner_uin = extract_id(url, "ownerUin=");
                    }
                    // csrfCode **只从真正的 API 请求里取**。带 `csrfCode=` 的控制台 URL
                    // 不止接口调用（SSO 跳转、埋点也会带），取第一个很容易拿到不是一套的那个，
                    // 存进去服务端就判 `code=9 登录态验证失败`。只认 /_api/ 与 /cgi/capi
                    // 这两条实际调接口的路径。
                    if csrf_code.is_empty()
                        && url.contains("csrfCode=")
                        && (url.contains("/_api/") || url.contains("/cgi/capi"))
                    {
                        csrf_code = extract(url, "csrfCode=").unwrap_or_default();
                    }
                }
            }
        } else if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
            if Some(id) == final_req {
                let c = build_cookie(&v);
                if !c.is_empty() {
                    cookie = c;
                }
                if !cookie_is_complete(&cookie) {
                    return Err("控制台会话没有建立完整（Cookie 缺 ownerUin），本次不覆盖已有会话，请重新登录".into());
                }
                break;
            }
            if Some(id) == cookie_req_id {
                let c = build_cookie(&v);
                if !c.is_empty() {
                    if c != last_snapshot {
                        last_snapshot = c.clone();
                        stable_since = Some(tokio::time::Instant::now());
                    }
                    cookie = c;
                }
                cookie_req_id = None;
            }
            if Some(id) == quick_probe_id {
                quick_probe_id = None;
                if let Some(s) = v["result"]["result"]["value"].as_str() {
                    if s.starts_with("{\"status\":\"found\"") {
                        if let Ok(obj) = serde_json::from_str::<Value>(s) {
                            if let (Some(x), Some(y)) = (obj["x"].as_f64(), obj["y"].as_f64()) {
                                quick_coords = Some((x.round() as i64, y.round() as i64));
                            }
                        }
                        if !quick_native_done {
                            quick_native_done = true;
                            quick_clicked_at = Some(tokio::time::Instant::now());
                            let cid = next_id;
                            next_id += 1;
                            let _ = send_cmd(&mut ws, cid, "Runtime.evaluate", json!({
                                "expression": QUICK_LOGIN_CLICK, "returnByValue": true
                            })).await;
                        }
                    }
                }
            }
        }
    }

    if uin.is_empty() {
        uin = cookie
            .split(';')
            .find_map(|p| {
                let p = p.trim();
                p.strip_prefix("uin=").map(|v| v.trim_start_matches('o').to_string())
            })
            .unwrap_or_default();
    }
    if owner_uin.is_empty() {
        owner_uin = cookie
            .split(';')
            .find_map(|p| {
                let p = p.trim();
                p.strip_prefix("ownerUin=").map(|v| {
                    v.trim_start_matches('O').trim_end_matches('G').to_string()
                })
            })
            .unwrap_or_default();
    }

    Ok(LoginResult {
        cookie,
        uin,
        owner_uin,
        csrf_code,
        auto_quick_clicked: quick_native_done || quick_mouse_done,
    })
}

/// 在控制台页面的上下文里执行一段 JS，返回其结果。
///
/// 用途：旧网关会拒绝"非浏览器发起"的请求——脚本（reqwest）直接发那条 dashboard 请求实测回
/// `code=1216 不合法的云 API 类型`，而**同一个页面自己发是 `code=0`**。差别不在令牌、不在载荷，
/// 而在"请求由谁发出"（浏览器 Cookie / TLS 指纹 / 来源）。退路就是让页面自己发，我们只取结果。
///
/// 返回：JS 表达式的结果（字符串原样返回，其它类型转成 JSON 文本）。
pub async fn eval_in_page(expr: &str, timeout: Duration) -> Result<String, String> {
    let ws_url = find_target().await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("连接调试端口失败: {}", e))?;
    let mut next_id = 1u64;
    let _ = send_cmd(&mut ws, next_id, "Runtime.enable", json!({})).await;
    next_id += 1;

    let id = next_id;
    send_cmd(
        &mut ws,
        id,
        "Runtime.evaluate",
        json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }),
    )
    .await?;

    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next()).await;
        let txt = match msg {
            Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(format!("调试通道错误: {}", e)),
            Ok(None) => return Err("控制台页面已关闭".into()),
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("id").and_then(|x| x.as_u64()) != Some(id) {
            continue;
        }
        let result = v.get("result").cloned().unwrap_or(Value::Null);
        if let Some(ex) = result.get("exceptionDetails") {
            let brief: String = ex.to_string().chars().take(300).collect();
            return Err(format!("页面里执行 JS 抛异常: {}", brief));
        }
        let val = result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(match val {
            Value::String(s) => s,
            other => other.to_string(),
        });
    }
    Err("在页面里执行 JS 超时".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookies(v: Value) -> Value {
        json!({ "result": { "cookies": v } })
    }

    /// 扁平化时要跳过空值、同名优先取控制台域。这条错了会自己制造"半套 Cookie"：
    /// `.tencent.com` 上的 `uin=`（空）把 `.cloud.tencent.com` 上的真值顶掉。
    #[test]
    fn build_cookie_skips_empty_and_prefers_console_domain() {
        let c = build_cookie(&cookies(json!([
            { "domain": ".tencent.com", "name": "uin", "value": "" },
            { "domain": ".cloud.tencent.com", "name": "uin", "value": "o765378326" },
            { "domain": ".tencent.com", "name": "ownerUin", "value": "O100G" },
            { "domain": ".example.com", "name": "nope", "value": "x" }
        ])));
        assert!(c.contains("uin=o765378326"), "{}", c);
        assert!(c.contains("ownerUin=O100G"), "{}", c);
        assert!(!c.contains("nope"), "非 tencent.com 域不该进 Cookie 头");
        assert!(!c.contains("uin=;"), "空值不该出现");
    }

    #[test]
    fn build_cookie_prefers_console_domain_regardless_of_order() {
        let c = build_cookie(&cookies(json!([
            { "domain": ".cloud.tencent.com", "name": "uin", "value": "real" },
            { "domain": ".tencent.com", "name": "uin", "value": "weaker" }
        ])));
        assert!(c.contains("uin=real"), "{}", c);
        assert!(!c.contains("weaker"), "{}", c);
    }

    #[test]
    fn cookie_has_requires_non_empty_value() {
        assert!(!cookie_has("uin=", "uin"), "空值不算有");
        assert!(!cookie_has("uin=; x=1", "uin"));
        assert!(cookie_has("uin=o1; x=1", "uin"));
        // "uin" 不能命中 "ownerUin"
        assert!(!cookie_has("ownerUin=O1G", "uin"));
        assert!(cookie_has("ownerUin=O1G", "owneruin"), "键名匹配应大小写不敏感");
    }

    #[test]
    fn cookie_is_complete_needs_uin_plus_owner_or_refresh() {
        assert!(!cookie_is_complete("ownerUin=O1G"), "只有 ownerUin 是半套");
        assert!(!cookie_is_complete("uin=o1"), "只有 uin 是半套");
        assert!(cookie_is_complete("uin=o1; ownerUin=O1G"));
        assert!(cookie_is_complete("uin=o1; refreshSession=r"));
    }

    /// 诊断日志只能打"有/无"，绝不能把 Cookie 值打进日志
    #[test]
    fn describe_cookie_never_leaks_values() {
        let d = describe_cookie("uin=o765378326; ownerUin=O100012781415G; saas_synced_session=SECRETVAL");
        assert!(d.starts_with("段数 3"), "{}", d);
        assert!(!d.contains("765378326"), "不能泄露值：{}", d);
        assert!(!d.contains("100012781415"), "不能泄露值：{}", d);
        assert!(!d.contains("SECRETVAL"), "不能泄露值：{}", d);
    }

    /// 控制台 URL 里 `ownerUin=0` 是占位值，真值在 Cookie 里
    #[test]
    fn extract_id_treats_zero_as_missing() {
        assert_eq!(extract_id("?ownerUin=0&x=1", "ownerUin="), "");
        assert_eq!(extract_id("?uin=o123&x=1", "uin="), "o123");
    }

    #[test]
    fn extract_stops_at_delimiter() {
        assert_eq!(extract("a&csrfCode=abc-1_2&b", "csrfCode=").as_deref(), Some("abc-1_2"));
        assert_eq!(extract("csrfCode=&b", "csrfCode="), None);
        assert_eq!(extract("no-such-key", "csrfCode="), None);
    }

    /// 抓包核过：x-life = 该请求的 t_ms - 会话常数；x-lid 会话内恒定、头名大小写不定。
    /// 这个函数是旧网关头信息抓取的纯函数版，测试钉住解析规则。
    #[test]
    fn capi_request_meta_reads_x_headers_case_insensitively() {
        // 与 tools/capture_dump.json 里 id=6774 同构：t=1790127636620, x-life=49560944
        let v = json!({
            "params": { "request": {
                "url": "https://console.cloud.tencent.com/cgi/capi?cmd=DescribeDashboardMetricData&action=delegate&serviceType=monitor&secure=1&version=3&dictId=2006&sts=1&t=1790127636620&uin=765378326&ownerUin=100012781415&csrfCode=1732802354",
                "headers": { "X-Lid": "sJxn-Y7DfV", "X-life": "49560944" }
            }}
        });
        let (lid, epoch) = capi_request_meta(&v);
        assert_eq!(lid, "sJxn-Y7DfV");
        assert_eq!(epoch, 1790127636620 - 49560944, "life_epoch = t_ms - x_life");
    }

    #[test]
    fn capi_request_meta_missing_headers_is_graceful() {
        let v = json!({
            "params": { "request": {
                "url": "https://console.cloud.tencent.com/cgi/capi?t=1790127636620&csrfCode=1",
                "headers": { "content-type": "application/json" }
            }}
        });
        let (lid, epoch) = capi_request_meta(&v);
        assert_eq!(lid, "");
        assert_eq!(epoch, 0, "缺 x-life 时 life_epoch 应为 0（发请求时跳过 x-life 头）");
    }

    #[test]
    fn capi_request_meta_ignores_non_capi_urls() {
        // t= 若出现在别的参数形态里不应误取；URL 不完整/没有 t 时 epoch 为 0
        let v = json!({
            "params": { "request": {
                "url": "https://console-hc.cloud.tencent.com/_api/monitor/GetMonitorData?t=99&uin=1",
                "headers": { "x-lid": "abc", "x-life": "123" }
            }}
        });
        let (lid, epoch) = capi_request_meta(&v);
        assert_eq!(lid, "abc");
        assert_eq!(epoch, 0, "x-life 在，但 URL 里没有 t= 可供对齐，无法算 epoch");
    }

    /// 抓包实锤的 csrf 算法：bkn(skey)。这里的样例值来自真实抓包
    /// （tools/capture_dump.json：skey 的 bkn = 1732802354 = 页面所有请求的 csrfCode）
    #[test]
    fn bkn_matches_capture_csrf() {
        let skey_from_capture = "JDf9UdfZDWrSYaYlU5lYZGoE3PwBB96Es4spg6UJf2U_";
        assert_eq!(bkn(skey_from_capture), 1732802354);
        // 恒等式：同物同值、空串也给一个确定值（不 panic）
        assert_eq!(bkn(skey_from_capture), bkn(skey_from_capture));
        let _ = bkn("");
    }
}
