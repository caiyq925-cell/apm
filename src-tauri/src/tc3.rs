// 腾讯云 API 3.0 TC3-HMAC-SHA256 签名实现（POST / JSON）
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::Duration;

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub struct Tc3Credential {
    pub secret_id: String,
    pub secret_key: String,
}

/// 调用腾讯云 OpenAPI，返回 Response 节点（业务错误会转成 Err）
pub async fn call_api(
    region: &str,
    service: &str,
    version: &str,
    action: &str,
    payload: &serde_json::Value,
    cred: &Tc3Credential,
) -> Result<serde_json::Value, String> {
    let host = format!("{}.tencentcloudapi.com", service);
    // 官方 API 不接受控制台专用字段（Version 走 X-TC-Version 头、Language/Region 等也不属于业务参数）
    let mut clean = payload.clone();
    if let Some(obj) = clean.as_object_mut() {
        for k in ["Version", "Language", "Region", "regionId", "SpaceUUID", "Module"] {
            obj.remove(k);
        }
    }
    let body = serde_json::to_string(&clean).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now();
    let ts = now.timestamp();
    let date = now.format("%Y-%m-%d").to_string();

    let canonical_request = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:{}\nx-tc-action:{}\n\ncontent-type;host;x-tc-action\n{}",
        host,
        action.to_lowercase(),
        sha256_hex(body.as_bytes())
    );
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{}\n{}/{}/tc3_request\n{}",
        ts,
        date,
        service,
        sha256_hex(canonical_request.as_bytes())
    );
    let k_date = hmac_sha256(format!("TC3{}", cred.secret_key).as_bytes(), date.as_bytes());
    let k_service = hmac_sha256(&k_date, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"tc3_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));
    let authorization = format!(
        "TC3-HMAC-SHA256 Credential={}/{}/{}/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature={}",
        cred.secret_id, date, service, signature
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("https://{}", host))
        .header("X-TC-Action", action)
        .header("X-TC-Version", version)
        .header("X-TC-Region", region)
        .header("X-TC-Timestamp", ts.to_string())
        .header("Authorization", authorization)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| format!("响应非 JSON (HTTP {}): {}", status, truncate(&text, 300)))?;

    if let Some(err) = v["Response"]["Error"].as_object() {
        let code = err.get("Code").and_then(|x| x.as_str()).unwrap_or("Unknown");
        let msg = err.get("Message").and_then(|x| x.as_str()).unwrap_or("");
        return Err(format!("腾讯云 API 错误 {}: {}", code, msg));
    }
    Ok(v["Response"].clone())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}...", &s[..n]) }
}
