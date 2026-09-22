import io

# ---------- apm.rs：恢复旧网关调用（call_capi），供容器指标使用 ----------
p = "src-tauri/src/apm.rs"
s = io.open(p, encoding="utf-8").read()
if "pub async fn call_capi" not in s:
    anchor = "}\n\n/// 实例级指标（APM「实例分析」）"
    capi = '''}

impl Channel {
    /// 控制台旧网关 /cgi/capi（容器监控 dashboard 指标等只有这条链路可用）
    /// 注意：该网关的 csrfCode/sts 会话令牌时效较短，过期会返回 code=1216
    pub async fn call_capi(&self, service: &str, cmd: &str, data: &Value) -> Result<Value, String> {
        match self {
            Channel::Secret { region, cred } => {
                call_api(region, service, "2018-07-24", cmd, data, cred).await
            }
            Channel::Cookie(cfg) => {
                let (uin, owner) = cfg.extract_ids_from_cookie();
                if uin.is_empty() || owner.is_empty() {
                    return Err("无法确定 uin/ownerUin：请检查 Cookie 或在设置中手动填写".into());
                }
                if cfg.csrf_code.is_empty() {
                    return Err("缺少 csrfCode：请在控制台复制一条请求的 cURL，用「解析并填充」更新".into());
                }
                let inner = json!({
                    "cmd": cmd,
                    "serviceType": service,
                    "data": data,
                    "regionId": cfg.region_id
                });
                let body = json!({
                    "text": inner.to_string(),
                    "mime": "application/json",
                    "encoding": "utf8"
                });
                let now_ms = chrono::Utc::now().timestamp_millis();
                let url = format!(
                    "https://console.cloud.tencent.com/cgi/capi?cmd={}&action=delegate&serviceType={}&secure=1&version=3&json=1&dictId=2006&sts=1&t={}&uin={}&ownerUin={}&csrfCode={}",
                    cmd, service, now_ms, uin, owner, cfg.csrf_code
                );
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .map_err(|e| e.to_string())?;
                let resp = client
                    .post(url)
                    .header("Content-Type", "application/json")
                    .header("Origin", "https://console.cloud.tencent.com")
                    .header("Referer", "https://console.cloud.tencent.com/tke2/cluster")
                    .header("X-Requested-With", "XMLHttpRequest")
                    .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36")
                    .header("Cookie", &cfg.cookie)
                    .body(body.to_string())
                    .send()
                    .await
                    .map_err(|e| format!("请求失败: {}", e))?;
                let text = resp.text().await.map_err(|e| e.to_string())?;
                let v: Value = serde_json::from_str(&text)
                    .map_err(|_| format!("响应非 JSON: {}", text.chars().take(200).collect::<String>()))?;
                if v["code"].as_i64().unwrap_or(-1) != 0 {
                    let hint = if v["code"].as_i64() == Some(1216) {
                        "（控制台会话令牌已过期：请在浏览器刷新一次控制台页面，复制任一请求的 cURL 到工具里「解析并填充」后重试）"
                    } else {
                        ""
                    };
                    return Err(format!("控制台接口错误 code={}: {}{}", v["code"], v["msg"].as_str().unwrap_or(""), hint));
                }
                let d = &v["data"];
                if d["data"]["Response"].is_object() {
                    Ok(d["data"]["Response"].clone())
                } else if d["Response"].is_object() {
                    Ok(d["Response"].clone())
                } else {
                    Ok(d["data"].clone())
                }
            }
        }
    }
}

/// 实例级指标（APM「实例分析」）'''
    assert anchor in s, "apm.rs 锚点未找到"
    s = s.replace(anchor, capi, 1)
    io.open(p, "w", encoding="utf-8").write(s)
    print("apm.rs ok")

# ---------- container.rs：dashboard 指标改走旧网关 ----------
p = "src-tauri/src/container.rs"
s = io.open(p, encoding="utf-8").read()
old = '''    let resp = ch
        .call_service("monitor", "2018-07-24", "DescribeDashboardMetricData", &payload)
        .await?;'''
new = '''    // 新网关对 QCE/TKE2 的 dashboard 查询返回空，必须走旧网关 /cgi/capi
    let resp = ch.call_capi("monitor", "DescribeDashboardMetricData", &payload).await?;'''
assert old in s, "container.rs 片段未找到"
s = s.replace(old, new, 1)
io.open(p, "w", encoding="utf-8").write(s)
print("container.rs ok")
