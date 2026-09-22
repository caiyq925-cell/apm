// 容器服务（TKE）数据源
// 链路：集群/命名空间用 TKE 接口；K8s 资源（Deployment/Pod、SW_AGENT_NAME、limit）走
// ForwardPlatformRequestV3 平台转发；容器利用率指标走云监控 DescribeDashboardMetricData（QCE/TKE2）
use crate::apm::Channel;
use serde_json::{json, Value};

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentInfo {
    pub name: String,
    pub namespace: String,
    /// 期望副本数
    pub replicas: i64,
    /// 就绪副本数
    pub ready: i64,
    /// 容器 env SW_AGENT_NAME（映射 APM 应用，严格相等）
    pub apm_name: String,
    /// 单 Pod 容器 limit 原样展示（如 "2" / "6Gi"）
    pub cpu_limit: String,
    pub mem_limit: String,
}

fn api_version_chain() -> [&'static str; 2] {
    ["/apis/apps/v1", "/apis/apps/v1beta2"]
}

/// 通过平台转发执行 K8s 请求，返回 ResponseBody 解析后的 JSON
async fn k8s_get(ch: &Channel, cluster_id: &str, path: &str) -> Result<Value, String> {
    let payload = json!({
        "Version": "2018-05-25",
        "Language": "zh-CN",
        "Method": "GET",
        "Path": path,
        "ClusterName": cluster_id
    });
    let resp = ch
        .call_service("tke", "2018-05-25", "ForwardPlatformRequestV3", &payload)
        .await?;
    let body = resp["ResponseBody"].as_str().unwrap_or("");
    if body.is_empty() {
        if let Some(err) = resp["Error"].as_object() {
            return Err(format!(
                "{}: {}",
                err.get("Code").and_then(|x| x.as_str()).unwrap_or(""),
                err.get("Message").and_then(|x| x.as_str()).unwrap_or("")
            ));
        }
        return Err("平台转发返回为空".into());
    }
    serde_json::from_str(body).map_err(|e| format!("解析 K8s 响应失败: {}", e))
}

pub async fn list_clusters(ch: &Channel) -> Result<Vec<ClusterInfo>, String> {
    let payload = json!({ "Version": "2018-05-25", "Limit": 100 });
    let resp = ch.call_service("tke", "2018-05-25", "DescribeClusters", &payload).await?;
    let mut out = Vec::new();
    if let Some(list) = resp["Clusters"].as_array() {
        for c in list {
            let id = c["ClusterId"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            out.push(ClusterInfo {
                id,
                name: c["ClusterName"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(out)
}

pub async fn list_namespaces(ch: &Channel, cluster_id: &str) -> Result<Vec<String>, String> {
    let payload = json!({ "Version": "2018-05-25", "ClusterId": cluster_id });
    let resp = ch
        .call_service("tke", "2018-05-25", "DescribeClusterNamespaces", &payload)
        .await?;
    let mut out = Vec::new();
    if let Some(list) = resp["Namespaces"].as_array() {
        for n in list {
            if let Some(name) = n["Name"].as_str() {
                if !name.is_empty() {
                    out.push(name.to_string());
                }
            }
        }
    }
    Ok(out)
}

fn env_value(container: &Value) -> String {
    container["env"]
        .as_array()
        .and_then(|envs| {
            envs.iter()
                .find(|e| e["name"].as_str() == Some("SW_AGENT_NAME"))
                .and_then(|e| e["value"].as_str())
        })
        .unwrap_or("")
        .to_string()
}

fn limits_of(container: &Value) -> (String, String) {
    let l = &container["resources"]["limits"];
    (
        l["cpu"].as_str().unwrap_or("").to_string(),
        l["memory"].as_str().unwrap_or("").to_string(),
    )
}

/// Deployment 列表（含 SW_AGENT_NAME 与单 Pod limit）
pub async fn list_deployments(
    ch: &Channel,
    cluster_id: &str,
    namespace: &str,
) -> Result<Vec<DeploymentInfo>, String> {
    let mut last_err = String::new();
    for base in api_version_chain() {
        let path = format!("/{}/namespaces/{}/deployments?limit=500", base.trim_start_matches('/'), namespace);
        match k8s_get(ch, cluster_id, &path).await {
            Ok(obj) => {
                let mut out = Vec::new();
                for d in obj["items"].as_array().unwrap_or(&vec![]) {
                    let name = d["metadata"]["name"].as_str().unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let containers = d["spec"]["template"]["spec"]["containers"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let mut apm = String::new();
                    let (mut cpu, mut mem) = (String::new(), String::new());
                    for c in &containers {
                        if apm.is_empty() {
                            apm = env_value(c);
                        }
                        let (c1, m1) = limits_of(c);
                        if cpu.is_empty() && !c1.is_empty() {
                            cpu = c1;
                        }
                        if mem.is_empty() && !m1.is_empty() {
                            mem = m1;
                        }
                    }
                    out.push(DeploymentInfo {
                        name,
                        namespace: namespace.to_string(),
                        replicas: d["spec"]["replicas"].as_i64().unwrap_or(0),
                        ready: d["status"]["readyReplicas"].as_i64().unwrap_or(0),
                        apm_name: apm,
                        cpu_limit: cpu,
                        mem_limit: mem,
                    });
                }
                out.sort_by(|a, b| a.name.cmp(&b.name));
                return Ok(out);
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 该 Deployment 的 Pod 名列表与就绪数（Pod 上的 limit 视为与 Deployment 模板一致）
pub async fn deployment_pods(
    ch: &Channel,
    cluster_id: &str,
    namespace: &str,
    deployment: &str,
) -> Result<(Vec<String>, i64), String> {
    for base in api_version_chain() {
        let path = format!(
            "/{}/namespaces/{}/deployments/{}/pods?limit=500",
            base.trim_start_matches('/'),
            namespace,
            deployment
        );
        if let Ok(obj) = k8s_get(ch, cluster_id, &path).await {
            let mut names = Vec::new();
            let mut ready = 0;
            for p in obj["items"].as_array().unwrap_or(&vec![]) {
                if let Some(n) = p["metadata"]["name"].as_str() {
                    names.push(n.to_string());
                }
                let is_ready = p["status"]["conditions"]
                    .as_array()
                    .map(|cs| cs.iter().any(|c| c["type"].as_str() == Some("Ready") && c["status"].as_str() == Some("True")))
                    .unwrap_or(false);
                if is_ready {
                    ready += 1;
                }
            }
            return Ok((names, ready));
        }
    }
    // 退化到按命名空间列 Pod 再按名字前缀过滤
    let path = format!("/api/v1/namespaces/{}/pods?limit=1000", namespace);
    let obj = k8s_get(ch, cluster_id, &path).await?;
    let mut names = Vec::new();
    let mut ready = 0;
    for p in obj["items"].as_array().unwrap_or(&vec![]) {
        let n = p["metadata"]["name"].as_str().unwrap_or("");
        if !n.starts_with(deployment) {
            continue;
        }
        names.push(n.to_string());
        let is_ready = p["status"]["conditions"]
            .as_array()
            .map(|cs| cs.iter().any(|c| c["type"].as_str() == Some("Ready") && c["status"].as_str() == Some("True")))
            .unwrap_or(false);
        if is_ready {
            ready += 1;
        }
    }
    Ok((names, ready))
}

fn iso_local(secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
        .unwrap_or_default()
}

fn dimension_json(region: &str, cluster_id: &str, pods: &[String]) -> String {
    let mut items = vec![
        json!({ "Key": "region", "Value": [region], "Operator": "eq" }),
        json!({ "Key": "tke_cluster_instance_id", "Value": [cluster_id], "Operator": "in" }),
    ];
    if !pods.is_empty() {
        items.push(json!({ "Key": "pod_name", "Value": pods, "Operator": "in" }));
    }
    json!(items).to_string()
}

/// 查询容器利用率指标，返回 (指标名, 最大百分比)
pub async fn pod_util_metrics(
    ch: &Channel,
    cluster_id: &str,
    pods: &[String],
    start: i64,
    end: i64,
    region: &str,
) -> Result<Vec<(String, f64)>, String> {
    const METRICS: [(&str, &str); 2] = [
        ("cpu_util_limit", "K8sPodRateCpuCoreUsedLimit"),
        ("mem_util_limit", "K8sPodRateMemNoCacheLimit"),
    ];
    let dims = dimension_json(region, cluster_id, pods);
    let queries: Vec<Value> = METRICS
        .iter()
        .map(|(_, m)| {
            json!({
                "Datasource": "DS_QCEMetric",
                "Namespace": "QCE/TKE2",
                "MetricName": m,
                "Conditions": [{ "Region": region, "Dimension": [dims] }],
                "GroupBy": ["InstanceId"],
                "StartTime": iso_local(start),
                "EndTime": iso_local(end),
                "QueryVersion": "2020-10-21"
            })
        })
        .collect();
    let payload = json!({
        "Version": "2018-07-24",
        "Language": "zh-CN",
        "SpaceUUID": "space_default",
        "Module": "monitor",
        "Query": queries
    });
    let resp = ch
        .call_service("monitor", "2018-07-24", "DescribeDashboardMetricData", &payload)
        .await?;

    let mut out: Vec<(String, f64)> = Vec::new();
    let data = resp["Data"].as_array().cloned().unwrap_or_default();
    for (key, metric) in METRICS {
        let mut max: Option<f64> = None;
        for d in &data {
            if d["MetricName"].as_str() != Some(metric) {
                continue;
            }
            // Value 是 JSON 数组字符串，如 "[1.6,null,1.8]"
            let raw = d["Value"].as_str().unwrap_or("");
            if let Ok(vals) = serde_json::from_str::<Vec<Option<f64>>>(raw) {
                for v in vals.into_iter().flatten() {
                    max = Some(max.map_or(v, |m: f64| m.max(v)));
                }
            }
        }
        if let Some(m) = max {
            out.push((key.to_string(), m));
        }
    }
    Ok(out)
}
