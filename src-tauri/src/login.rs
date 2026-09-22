// 内嵌登录：打开腾讯云官方登录页（微信扫码），登录后经 CDP 取回会话信息写入配置。
// 全程在腾讯官方页面完成扫码，工具不接触账号密码。
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

pub const CDP_PORT: u16 = 9223;
pub const LOGIN_URL: &str = "https://console.cloud.tencent.com/monitor/apm/system/list";

pub struct LoginResult {
    pub cookie: String,
    pub uin: String,
    pub owner_uin: String,
    pub csrf_code: String,
}

fn extract(url: &str, key: &str) -> Option<String> {
    let idx = url.find(key)?;
    let rest = &url[idx + key.len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .unwrap_or(rest.len());
    if end == 0 { None } else { Some(rest[..end].to_string()) }
}

/// 等待登录窗口的调试目标出现，返回 WS 地址
async fn find_target() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    for _ in 0..120 {
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
    Err("未能连接登录窗口的调试端口，请确认登录窗口已打开".into())
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
    let mut parts: Vec<String> = Vec::new();
    if let Some(cookies) = v["result"]["cookies"].as_array() {
        for c in cookies {
            let domain = c.get("domain").and_then(|d| d.as_str()).unwrap_or("");
            if !domain.ends_with("tencent.com") {
                continue;
            }
            let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let value = c.get("value").and_then(|x| x.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let kv = format!("{}={}", name, value);
            if !parts.contains(&kv) {
                parts.push(kv);
            }
        }
    }
    parts.join("; ")
}

/// 连接登录窗口，等待用户扫码登录，返回会话信息
pub async fn wait_for_login() -> Result<LoginResult, String> {
    let ws_url = find_target().await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("连接调试端口失败: {}", e))?;

    let mut next_id = 1u64;
    let _ = send_cmd(&mut ws, next_id, "Network.enable", json!({})).await;
    next_id += 1;
    let _ = send_cmd(&mut ws, next_id, "Page.navigate", json!({ "url": LOGIN_URL })).await;
    next_id += 1;

    let mut csrf_code = String::new();
    let mut uin = String::new();
    let mut owner_uin = String::new();
    let mut cookie = String::new();
    let mut cookie_req_id: Option<u64> = None;
    let mut last_cookie_poll = tokio::time::Instant::now();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err("等待登录超时：请在登录窗口中完成扫码登录".into());
        }
        // 已经拿到 csrfCode 但还没 Cookie 时，定期轮询 Cookie
        if !csrf_code.is_empty() && cookie.is_empty() && last_cookie_poll.elapsed() > Duration::from_secs(2) {
            let id = next_id;
            next_id += 1;
            cookie_req_id = Some(id);
            let _ = send_cmd(&mut ws, id, "Network.getAllCookies", json!({})).await;
            last_cookie_poll = tokio::time::Instant::now();
        }

        let msg = tokio::time::timeout(Duration::from_secs(3), ws.next()).await;
        let txt = match msg {
            Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(format!("调试通道错误: {}", e)),
            Ok(None) => return Err("登录窗口已关闭".into()),
            Err(_) => {
                if !csrf_code.is_empty() && !cookie.is_empty() {
                    break;
                }
                continue;
            }
        };
        let v: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if v.get("method").and_then(|m| m.as_str()) == Some("Network.requestWillBeSent") {
            if let Some(url) = v["params"]["request"]["url"].as_str() {
                if url.contains("csrfCode=") && url.contains("tencent.com") {
                    if csrf_code.is_empty() {
                        csrf_code = extract(url, "csrfCode=").unwrap_or_default();
                        uin = extract(url, "uin=").unwrap_or_default();
                        owner_uin = extract(url, "ownerUin=").unwrap_or_default();
                    }
                }
            }
        } else if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
            if Some(id) == cookie_req_id {
                let c = build_cookie(&v);
                if !c.is_empty() {
                    cookie = c;
                }
                cookie_req_id = None;
            }
        }

        if !csrf_code.is_empty() && !cookie.is_empty() {
            break;
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

    Ok(LoginResult { cookie, uin, owner_uin, csrf_code })
}
