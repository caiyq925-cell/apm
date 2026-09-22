// 从 Reqable 抓包中自动提取控制台登录信息（cookie / uin / ownerUin / csrfCode）
// 原理：拉起 Reqable 官方 MCP 服务（stdio），调用其抓包查询工具，取最近一条含 csrfCode 的控制台请求
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct LoginInfo {
    pub cookie: String,
    pub uin: String,
    pub owner_uin: String,
    pub csrf_code: String,
    #[allow(dead_code)]
    pub source: String,
}

fn mcp_path(cfg_path: &str) -> String {
    if !cfg_path.trim().is_empty() {
        return cfg_path.to_string();
    }
    for p in [
        r"C:\Program Files\Reqable\mcp-server.exe",
        r"C:\Program Files (x86)\Reqable\mcp-server.exe",
        r"D:\Program Files\Reqable\mcp-server.exe",
    ] {
        if std::path::Path::new(p).exists() {
            return p.to_string();
        }
    }
    r"C:\Program Files\Reqable\mcp-server.exe".to_string()
}

struct McpClient {
    child: tokio::process::Child,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl McpClient {
    async fn start(exe: &str, port: u16) -> Result<McpClient, String> {
        if !std::path::Path::new(exe).exists() {
            return Err(format!("未找到 Reqable MCP 服务：{}", exe));
        }
        let mut child = tokio::process::Command::new(exe)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--scope", "minimal"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("启动 Reqable MCP 失败: {}", e))?;
        let stdout = child.stdout.take().ok_or("无法读取 MCP 输出")?;
        let mut c = McpClient { child, reader: BufReader::new(stdout), next_id: 1 };
        c.request("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "apm-monitor", "version": "1.0"}
        }))
        .await?;
        c.notify("notifications/initialized", json!({})).await?;
        Ok(c)
    }

    async fn write_line(&mut self, v: &Value) -> Result<(), String> {
        let stdin = self.child.stdin.as_mut().ok_or("MCP stdin 不可用")?;
        let mut line = v.to_string();
        line.push('\n');
        stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_line(&json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_line(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await?;
        // 读到自己 id 的响应（跳过通知）
        loop {
            let mut line = String::new();
            let n = tokio::time::timeout(Duration::from_secs(25), self.reader.read_line(&mut line))
                .await
                .map_err(|_| "读取 MCP 响应超时".to_string())?
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("MCP 服务已退出".into());
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(|x| x.as_u64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(format!("MCP 调用失败: {}", err));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    async fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let res = self.request("tools/call", json!({ "name": name, "arguments": args })).await?;
        let text = res
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        serde_json::from_str(&text).map_err(|_| text)
    }
}

fn pick(v: &Value, key: &str) -> Option<String> {
    let re = key;
    v.as_str().map(|s| {
        let idx = s.find(re)?;
        let rest = &s[idx + re.len()..];
        let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    })?
}

/// 从 Reqable 抓包取最近一条含 csrfCode 的控制台请求，解析登录信息
pub async fn fetch_login_info(exe: &str, port: u16) -> Result<LoginInfo, String> {
    let mut c = McpClient::start(&mcp_path(exe), port).await?;
    let _ = c.call_tool("capture_live_set_enabled", json!({ "enabled": true })).await;

    let ids = c
        .call_tool("capture_live_filter", json!({ "filters": [{ "type": "keyword", "pattern": "csrfCode" }] }))
        .await?;
    let mut list: Vec<i64> = Vec::new();
    match &ids {
        Value::Array(arr) => {
            for it in arr {
                match it {
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            list.push(i)
                        }
                    }
                    Value::Object(o) => {
                        if let Some(i) = o.get("id").and_then(|x| x.as_i64()) {
                            list.push(i)
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => return Err("Reqable 未返回抓包记录（请确认 Reqable 正在运行且开启抓包，并已打开过腾讯云控制台页面）".into()),
    }
    if list.is_empty() {
        return Err("Reqable 抓包里没有找到控制台请求（请先在浏览器打开一次腾讯云控制台页面）".into());
    }

    // 从最新往回找，挑出带 Cookie 与 csrfCode 的那条
    for id in list.iter().rev().take(40) {
        let rec = match c.call_tool("capture_live_get_by_id", json!({ "id": id })).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let req = rec.get("request").cloned().unwrap_or(Value::Null);
        let url = rec.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string();
        let path = req.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
        let full = if url.is_empty() { path.clone() } else { url.clone() };
        if !full.contains("csrfCode") || !full.contains("tencent.com") {
            continue;
        }
        let mut cookies: Vec<String> = Vec::new();
        if let Some(hs) = req.get("headers").and_then(|h| h.as_array()) {
            for h in hs {
                if h.get("name").and_then(|n| n.as_str()).map(|n| n.eq_ignore_ascii_case("cookie")) == Some(true) {
                    if let Some(v) = h.get("value").and_then(|v| v.as_str()) {
                        cookies.push(v.trim().to_string());
                    }
                }
            }
        }
        if cookies.is_empty() {
            continue;
        }
        let csrf = pick(&Value::String(full.clone()), "csrfCode=").unwrap_or_default();
        let uin = pick(&Value::String(full.clone()), "uin=").unwrap_or_default();
        let owner = pick(&Value::String(full.clone()), "ownerUin=").unwrap_or_default();
        if csrf.is_empty() || uin.is_empty() {
            continue;
        }
        return Ok(LoginInfo {
            cookie: cookies.join("; "),
            uin,
            owner_uin: owner,
            csrf_code: csrf,
            source: full.chars().take(160).collect(),
        });
    }
    Err("抓包里没能解析出 Cookie/csrfCode（请先在浏览器打开一次腾讯云控制台页面）".into())
}
