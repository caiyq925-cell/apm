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

/// dashboard 接口的维度串。
///
/// **必须与控制台页面自己的请求逐字一致**（抓包核过，见 `tools/capture_dump.json`）：
/// 控制台对容器利用率固定带 5 个维度 —— region / tke_cluster_instance_id / pod_name /
/// **namespace** / **workload_name**。少一个都可能取不到数据；这里缺失的那两个
/// 正是"容器利用率一直拿不到"的怀疑对象（接口本身是通的：抓包里
/// `K8sPodRateCpuCoreUsedLimit` 的响应是 `code=0` 且有真实百分比）。
fn dimension_json(region: &str, cluster_id: &str, namespace: &str, workload: &str, pods: &[String]) -> String {
    let mut items = vec![
        json!({ "Key": "region", "Value": [region], "Operator": "eq" }),
        json!({ "Key": "tke_cluster_instance_id", "Value": [cluster_id], "Operator": "in" }),
    ];
    if !pods.is_empty() {
        items.push(json!({ "Key": "pod_name", "Value": pods, "Operator": "in" }));
    }
    if !namespace.is_empty() {
        items.push(json!({ "Key": "namespace", "Value": [namespace], "Operator": "eq" }));
    }
    if !workload.is_empty() {
        items.push(json!({ "Key": "workload_name", "Value": [workload], "Operator": "eq" }));
    }
    json!(items).to_string()
}

/// 与控制台一致的采样周期（QCE 只接受标准周期）。
/// 控制台对 1 小时窗口用的就是 60（抓包核过），所以短窗口直接对齐；
/// 长窗口必须放粗，否则 7 天 × 60s 会拉回上万个点。
fn pick_period(start: i64, end: i64) -> i64 {
    let secs = (end - start).max(0);
    if secs <= 3600 {
        60
    } else if secs <= 6 * 3600 {
        300
    } else if secs <= 24 * 3600 {
        3600
    } else {
        86400
    }
}

/// 容器利用率两个指标：(输出键, dashboard 指标名)
const UTIL_METRICS: [(&str, &str); 2] = [
    ("cpu_util_limit", "K8sPodRateCpuCoreUsedLimit"),
    ("mem_util_limit", "K8sPodRateMemNoCacheLimit"),
];

/// 从 dashboard 响应的 `Data[]` 里取出两个利用率指标在窗口内的最大值。
///
/// 注意响应里 `Value` 是**字符串形式的 JSON 数组**（如 `"[1.6,null,1.8]"`），可能含 null。
/// 抽成纯函数是为了能测——这段解析错了会静默变成"没有数据"，很难查。
fn parse_util_data(data: &[Value]) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for (key, metric) in UTIL_METRICS {
        let mut max: Option<f64> = None;
        for d in data {
            if d["MetricName"].as_str() != Some(metric) {
                continue;
            }
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
    out
}

/// 查询容器利用率指标，返回 (指标名, 最大百分比)
///
/// 旧网关走**配置自助**：csrfCode = bkn(skey) 现算、Cookie 用配置会话（见 `apm::call_capi`），
/// 不需要浏览器/预热窗口。脚本直发实测 `code=0` 且有数据；万一被网关拒（`is_capi_token_stale`），
/// 才用 `app` 走退路——让隐藏的控制台页面自己发（见 `capi::fetch_dashboard`）。
pub async fn pod_util_metrics(
    ch: &Channel,
    app: Option<&tauri::AppHandle>,
    cluster_id: &str,
    namespace: &str,
    workload: &str,
    pods: &[String],
    start: i64,
    end: i64,
    region: &str,
) -> Result<Vec<(String, f64)>, String> {
    let dims = dimension_json(region, cluster_id, namespace, workload, pods);
    let period = pick_period(start, end);
    let queries: Vec<Value> = UTIL_METRICS
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
                // 控制台自己带 Period；缺了它取不到数据（抓包核过）
                "Period": period,
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
    // 只有控制台旧网关 dashboard 接口能给出容器利用率（官方 API 对 QCE/TKE2 无数据，已实测）
    match ch
        .call_capi("monitor", "DescribeDashboardMetricData", &payload)
        .await
    {
        Ok(resp) => {
            let data = resp["Data"].as_array().cloned().unwrap_or_default();
            Ok(parse_util_data(&data))
        }
        Err(e) if crate::cred::is_capi_token_stale(&e) => {
            // 脚本直发被网关拒 → 退到"让页面自己发"。只在网关拒了我们时走这条路：
            // 网络类错误走它也没用，白等 30 秒。
            let Some(a) = app else { return Err(e) };
            match crate::capi::fetch_dashboard(a, ch.config(), "monitor", "DescribeDashboardMetricData", &payload).await {
                Ok(data) => {
                    let arr = data.as_array().cloned().unwrap_or_default();
                    let out = parse_util_data(&arr);
                    if out.is_empty() {
                        Err(format!("{}；退路：页面里发成功了但没解析出数据", e))
                    } else {
                        Ok(out)
                    }
                }
                Err(e2) => Err(format!("{}；退路也失败：{}", e, e2)),
            }
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 控制台自己的 dashboard 请求固定带这 5 个维度（tools/capture_dump.json 核过）。
    /// 少维度是"容器利用率一直拿不到"的头号怀疑对象，所以钉死在这里。
    #[test]
    fn dimension_json_has_all_five_dims_the_console_sends() {
        let s = dimension_json(
            "ap-shanghai",
            "cls-abc",
            "inc",
            "inc-center",
            &["p1".to_string(), "p2".to_string()],
        );
        let v: Value = serde_json::from_str(&s).unwrap();
        let keys: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["Key"].as_str().unwrap())
            .collect();
        assert_eq!(
            keys,
            vec!["region", "tke_cluster_instance_id", "pod_name", "namespace", "workload_name"]
        );
        assert_eq!(v[0]["Operator"], "eq");
        assert_eq!(v[1]["Operator"], "in");
        assert_eq!(v[2]["Value"], json!(["p1", "p2"]));
        assert_eq!(v[3]["Value"], json!(["inc"]));
        assert_eq!(v[4]["Value"], json!(["inc-center"]));
    }

    #[test]
    fn dimension_json_omits_empty_optional_dims() {
        let s = dimension_json("ap-shanghai", "cls-abc", "", "", &[]);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2, "只该剩 region + cluster");
    }

    /// 控制台对 1 小时窗口用的就是 60（抓包核过）；长窗口必须放粗，否则点数爆炸
    #[test]
    fn period_matches_console_for_short_windows() {
        assert_eq!(pick_period(0, 3600), 60);
        assert_eq!(pick_period(0, 6 * 3600), 300);
        assert_eq!(pick_period(0, 24 * 3600), 3600);
        assert_eq!(pick_period(0, 7 * 24 * 3600), 86400);
        // 反向/零窗口不应 panic
        assert_eq!(pick_period(100, 0), 60);
    }

    /// 响应里 `Value` 是**字符串形式的 JSON 数组**（可能含 null），取窗口内所有点的最大值
    #[test]
    fn parse_util_data_takes_max_across_points() {
        let resp = json!({ "Data": [
            { "MetricName": "K8sPodRateCpuCoreUsedLimit", "Value": "[1.6,null,1.8]" },
            { "MetricName": "K8sPodRateCpuCoreUsedLimit", "Value": "[0.4]" },
            { "MetricName": "K8sPodRateMemNoCacheLimit", "Value": "[55.5,60.1]" },
            { "MetricName": "K8sPodRestartTotal", "Value": "[999]" }
        ]});
        let data = resp["Data"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            parse_util_data(&data),
            vec![
                ("cpu_util_limit".to_string(), 1.8),
                ("mem_util_limit".to_string(), 60.1),
            ]
        );
    }

    #[test]
    fn parse_util_data_empty_when_metric_missing_or_unparsable() {
        let other = json!([{ "MetricName": "K8sPodRestartTotal", "Value": "[1]" }]);
        assert!(parse_util_data(other.as_array().unwrap()).is_empty());

        let blank = json!([{ "MetricName": "K8sPodRateCpuCoreUsedLimit", "Value": "" }]);
        assert!(parse_util_data(blank.as_array().unwrap()).is_empty());

        assert!(parse_util_data(&[]).is_empty());
    }
}
