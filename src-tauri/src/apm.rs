// 腾讯云 APM 数据访问层：统一封装「密钥模式（官方 OpenAPI）」与「Cookie 模式（控制台私有接口）」
// 文档: https://cloud.tencent.com/document/api/1463/64936
use crate::config::{Config, MetricDef};
use crate::console;
use crate::tc3::{call_api, Tc3Credential};
use serde_json::{json, Value};

pub enum Channel {
    Secret { region: String, cred: Tc3Credential },
    Cookie(Config),
}

impl Channel {
    pub fn from_config(cfg: &Config) -> Result<Channel, String> {
        match cfg.auth_mode.as_str() {
            "cookie" => {
                if cfg.cookie.trim().is_empty() {
                    return Err("Cookie 模式未配置 Cookie".into());
                }
                Ok(Channel::Cookie(cfg.clone()))
            }
            _ => {
                if cfg.secret_id.is_empty() || cfg.secret_key.is_empty() {
                    return Err("未配置 SecretId/SecretKey，请先在设置中填写并保存".into());
                }
                Ok(Channel::Secret {
                    region: cfg.region.clone(),
                    cred: Tc3Credential { secret_id: cfg.secret_id.clone(), secret_key: cfg.secret_key.clone() },
                })
            }
        }
    }

    async fn call(&self, action: &str, payload: &Value) -> Result<Value, String> {
        match self {
            Channel::Secret { region, cred } => {
                call_api(region, "apm", "2021-06-22", action, payload, cred).await
            }
            Channel::Cookie(cfg) => console::call_console(cfg, "apm", "2021-06-22", action, payload.clone()).await,
        }
    }

    /// 通用服务调用（APM 之外的产品：monitor/cdb/redis/mongodb/dbbrain）
    pub async fn call_service(
        &self,
        service: &str,
        version: &str,
        action: &str,
        payload: &Value,
    ) -> Result<Value, String> {
        match self {
            Channel::Secret { region, cred } => {
                call_api(region, service, version, action, payload, cred).await
            }
            Channel::Cookie(cfg) => {
                console::call_console(cfg, service, version, action, payload.clone()).await
            }
        }
    }
}

/// 从 {Tags:.., Fields:..} 兼容「对象」或「[{Key,Value}]」两种形态取键值对
fn kv_map(v: &Value) -> std::collections::BTreeMap<String, Value> {
    let mut out = std::collections::BTreeMap::new();
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                out.insert(k.clone(), val.clone());
            }
        }
        Value::Array(arr) => {
            for item in arr {
                let k = item.get("Key").or_else(|| item.get("key")).and_then(|x| x.as_str());
                let val = item.get("Value").or_else(|| item.get("value"));
                if let (Some(k), Some(val)) = (k, val) {
                    out.insert(k.to_string(), val.clone());
                }
            }
        }
        _ => {}
    }
    out
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// 应用（服务）列表 + 请求量，按请求量降序
/// 实测（控制台通道）：DescribeApmServiceMetric 不带时间参数时一次性返回全部应用，
/// 带分页/时间参数反而返回空或报错
pub async fn list_apps(ch: &Channel, instance_id: &str) -> Result<Vec<(String, f64)>, String> {
    if instance_id.is_empty() {
        return Err("未配置业务系统 ID".into());
    }
    let payload = json!({ "InstanceId": instance_id, "PageSize": 10 });
    let resp = ch.call("DescribeApmServiceMetric", &payload).await?;
    let mut apps: Vec<(String, f64)> = Vec::new();
    if let Some(list) = resp["ServiceMetricList"].as_array() {
        for item in list {
            let tags = kv_map(&item["Tags"]);
            let fields = kv_map(&item["Fields"]);
            let name = tags
                .get("service.name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let name = match name {
                Some(n) if !n.is_empty() => n,
                _ => continue,
            };
            // 总请求数字段：request_count_sum（ServiceMetric）/ service_request_count_sum（Overview）
            let count = fields
                .iter()
                .filter(|(k, _)| k.contains("request_count"))
                .filter_map(|(_, v)| value_as_f64(v))
                .find(|v| *v > 0.0)
                .or_else(|| {
                    fields
                        .iter()
                        .filter(|(k, _)| k.contains("request_count"))
                        .find_map(|(_, v)| value_as_f64(v))
                })
                .unwrap_or(0.0);
            apps.push((name, count));
        }
    }
    apps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(apps)
}

/// 内置指标清单（名称均经控制台接口实测确认）
/// 注：官方 DescribeGeneralMetricList 在 apm/2021-06-22 下不存在，故使用实测清单
pub fn builtin_metric_defs() -> Vec<MetricDef> {
    [
        ("request_count", "请求量", "service_metric"),
        ("error_request_count", "异常请求量", "service_metric"),
        ("error_req_rate_avg", "错误率", "service_metric"),
        ("duration_p99", "P99耗时", "service_metric"),
        ("duration_p95", "P95耗时", "service_metric"),
        ("duration_avg", "平均耗时", "service_metric"),
        ("duration_max", "最大耗时", "service_metric"),
        ("duration_min", "最小耗时", "service_metric"),
        ("slow_request_count", "慢调用量", "service_metric"),
        ("tolerate_request_count", "容忍调用量", "service_metric"),
        // 计算型指标：不在服务端指标接口中（实测 qps 等名均非法），由前端用 请求量÷窗口秒数 计算
        ("qps_avg", "平均(次/秒)", "computed"),
    ]
    .iter()
    .map(|(name, cn, view)| MetricDef { name: name.to_string(), view: view.to_string(), cn: cn.to_string() })
    .collect()
}

/// 查询单应用在指定视图下的多个指标（Period=0，整段聚合，DataSerial 仅一个值）
pub async fn fetch_app_metrics(
    ch: &Channel,
    instance_id: &str,
    app: &str,
    view: &str,
    metrics: &[String],
    start_sec: i64,
    end_sec: i64,
) -> Result<Vec<(String, f64)>, String> {
    let payload = json!({
        "InstanceId": instance_id,
        "ViewName": view,
        "Metrics": metrics,
        // span.kind=server 对齐控制台口径：只统计服务端跨度，排除出站调用（client）
        "Filters": [
            { "Key": "service.name", "Value": app },
            { "Key": "span.kind", "Value": "server" }
        ],
        "GroupBy": ["service.name"],
        "StartTime": start_sec,
        "EndTime": end_sec,
        "Period": 0,
        "PageSize": 50
    });
    let resp = ch.call("DescribeGeneralMetricData", &payload).await?;
    let mut raw: Vec<(String, f64)> = Vec::new();
    if let Some(records) = resp["Records"].as_array() {
        for r in records {
            // 实测两种形态：Records[i].Line[j]（文档描述）与 Records[i] 本身即一行（控制台实际返回）
            if let Some(lines) = r["Line"].as_array() {
                for line in lines {
                    parse_metric_line(line, &mut raw);
                }
            } else {
                parse_metric_line(r, &mut raw);
            }
        }
    }

    // 服务端会把个别指标名映射后返回（如 request_count → service_request_count_sum），
    // 记录数与请求数一致时按位置对齐回请求名；否则按返回名归一别名
    let mut out: Vec<(String, f64)> = Vec::new();
    if raw.len() == metrics.len() {
        for (i, (_, v)) in raw.iter().enumerate() {
            out.push((metrics[i].clone(), *v));
        }
    } else {
        for (n, v) in raw {
            let name = if n == "service_request_count_sum" { "request_count".to_string() } else { n };
            out.push((name, v));
        }
    }
    Ok(out)
}

fn parse_metric_line(line: &Value, out: &mut Vec<(String, f64)>) {
    let name = line["MetricName"].as_str().unwrap_or("");
    if name.is_empty() {
        return;
    }
    // DataSerial 可能含 null（无数据），取最后一个可解析值
    let val = line["DataSerial"]
        .as_array()
        .and_then(|s| s.iter().rev().find_map(value_as_f64))
        .or_else(|| value_as_f64(&line["DataSerial"]));
    if let Some(v) = val {
        out.push((name.to_string(), v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_console_records() {
        // 控制台通道实测形态：Records[i] 本身即一行，无 Line 包装
        let v: Value = serde_json::from_str(
            r#"{"Records":[
                {"Tags":[{"Key":"service.name","Value":"prd_iam-account"}],"MetricName":"service_request_count_sum","MetricNameCN":"总请求数","TimeSerial":[],"DataSerial":[18436695]},
                {"Tags":[{"Key":"service.name","Value":"prd_iam-account"}],"MetricName":"duration_max","TimeSerial":[],"DataSerial":[1438]}
            ]}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        if let Some(records) = v["Records"].as_array() {
            for r in records {
                if let Some(lines) = r["Line"].as_array() {
                    for line in lines {
                        parse_metric_line(line, &mut out);
                    }
                } else {
                    parse_metric_line(r, &mut out);
                }
            }
        }
        assert_eq!(
            out,
            vec![
                ("service_request_count_sum".to_string(), 18436695.0),
                ("duration_max".to_string(), 1438.0)
            ]
        );
    }

    #[test]
    fn parses_line_wrapped_records() {
        // OpenAPI 文档形态：Records[i].Line[j]
        let v: Value = serde_json::from_str(
            r#"{"Records":[{"Line":[{"MetricName":"request_count","DataSerial":[18, null, 5]}]}]}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        for r in v["Records"].as_array().unwrap() {
            for line in r["Line"].as_array().unwrap() {
                parse_metric_line(line, &mut out);
            }
        }
        assert_eq!(out, vec![("request_count".to_string(), 5.0)]);
    }
}
