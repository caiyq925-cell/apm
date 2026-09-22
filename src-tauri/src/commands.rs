// Tauri 命令层
use crate::apm::{self, Channel};
use crate::config::{Config, MetricDef};
use serde::Serialize;
use std::sync::Arc;

#[tauri::command]
pub fn load_config() -> Config {
    Config::load()
}

#[tauri::command]
pub fn save_config(config: Config) -> Result<(), String> {
    config.save()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub request_count: f64,
}

#[tauri::command]
pub async fn list_apps(config: Config) -> Result<Vec<AppInfo>, String> {
    let ch = Channel::from_config(&config)?;
    let apps = apm::list_apps(&ch, &config.instance_id).await?;
    Ok(apps
        .into_iter()
        .map(|(name, count)| AppInfo { name, request_count: count })
        .collect())
}

#[tauri::command]
pub async fn refresh_metric_defs(_config: Config) -> Result<Vec<MetricDef>, String> {
    // 官方无指标清单接口（DescribeGeneralMetricList 不存在于 apm/2021-06-22），返回实测内置清单
    Ok(apm::builtin_metric_defs())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MetricValue {
    pub name: String,
    pub value: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppMetrics {
    pub name: String,
    pub values: Vec<MetricValue>, // 按请求时的指标顺序
    pub error: Option<String>,
}

#[tauri::command]
pub async fn query_metrics(
    config: Config,
    start_ts: i64,
    end_ts: i64,
    apps: Vec<String>,
    metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {
    if config.instance_id.is_empty() {
        return Err("未配置业务系统 ID".into());
    }
    if apps.is_empty() {
        return Err("未选择应用".into());
    }
    if metrics.is_empty() {
        return Err("未选择指标".into());
    }
    let ch = Arc::new(Channel::from_config(&config)?);

    // 按视图分组指标，同一应用同一视图一次请求；computed=前端自算，instance_metric=实例级（走旧网关）
    let mut by_view: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut instance_metrics: Vec<String> = Vec::new();
    for m in &metrics {
        if m.view == "computed" {
            continue;
        }
        if m.view == "instance_metric" {
            instance_metrics.push(m.name.clone());
            continue;
        }
        by_view.entry(m.view.clone()).or_default().push(m.name.clone());
    }

    let sem = Arc::new(tokio::sync::Semaphore::new(8));
    let mut join = tokio::task::JoinSet::new();
    for app in &apps {
        for (view, names) in &by_view {
            let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
            let ch = ch.clone();
            let app = app.clone();
            let view = view.clone();
            let names = names.clone();
            let instance = config.instance_id.clone();
            join.spawn(async move {
                let r =
                    apm::fetch_app_metrics(&ch, &instance, &app, &view, &names, start_ts, end_ts).await;
                drop(permit);
                (app, r)
            });
        }
        // 实例级指标（实例分析）：独立接口，多实例取最大
        if !instance_metrics.is_empty() {
            let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
            let ch = ch.clone();
            let app = app.clone();
            let instance = config.instance_id.clone();
            let want = instance_metrics.clone();
            join.spawn(async move {
                let r =
                    apm::fetch_instance_tops(&ch, &instance, &app, start_ts, end_ts, &want).await;
                drop(permit);
                (app, r)
            });
        }
    }

    // app -> (metricName -> value)
    let mut collected: std::collections::HashMap<String, std::collections::HashMap<String, f64>> =
        Default::default();
    let mut app_err: std::collections::HashMap<String, String> = Default::default();
    while let Some(res) = join.join_next().await {
        match res {
            Ok((app, Ok(vals))) => {
                let e = collected.entry(app).or_default();
                for (k, v) in vals {
                    e.insert(k, v);
                }
            }
            Ok((app, Err(err))) => {
                // 同应用多视图部分成功时保留非空错误
                app_err.entry(app).or_insert(err);
            }
            Err(e) => return Err(format!("任务异常: {}", e)),
        }
    }

    let mut out = Vec::new();
    for app in &apps {
        let m = collected.remove(app).unwrap_or_default();
        let values = metrics
            .iter()
            .filter_map(|d| m.get(&d.name).map(|v| MetricValue { name: d.name.clone(), value: *v }))
            .collect();
        out.push(AppMetrics { name: app.clone(), values, error: app_err.get(app).cloned() });
    }
    Ok(out)
}

/// Cookie 模式校验：真实调用一次应用概览接口
#[tauri::command]
pub async fn validate_cookie(config: Config) -> Result<String, String> {
    if config.cookie.trim().is_empty() {
        return Err("请先粘贴 Cookie".into());
    }
    if config.instance_id.is_empty() {
        return Err("请先填写业务系统 ID".into());
    }
    let ch = Channel::Cookie(config.clone());
    let apps = apm::list_apps(&ch, &config.instance_id).await?;
    Ok(format!("校验通过，当前业务系统下可见 {} 个应用", apps.len()))
}

// ---------- 数据库监控 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbInstanceDto {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub async fn list_db_instances(config: Config, db_type: String) -> Result<Vec<DbInstanceDto>, String> {
    let ch = Channel::from_config(&config)?;
    let list = crate::db::list_db_instances(&ch, &db_type).await?;
    Ok(list
        .into_iter()
        .map(|d| DbInstanceDto { id: d.id, name: d.name })
        .collect())
}

#[tauri::command]
pub async fn query_db_metrics(
    config: Config,
    db_type: String,
    start_ts: i64,
    end_ts: i64,
    instances: Vec<String>,
    metrics: Vec<String>,
) -> Result<Vec<AppMetrics>, String> {
    if instances.is_empty() {
        return Err("未选择数据库实例".into());
    }
    if metrics.is_empty() {
        return Err("未选择指标".into());
    }
    let ch = Arc::new(Channel::from_config(&config)?);
    // 实例 ID -> 实例名（展示用）
    let name_map: std::collections::HashMap<String, String> =
        crate::db::list_db_instances(&ch, &db_type)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();

    let sem = Arc::new(tokio::sync::Semaphore::new(6));
    let mut join = tokio::task::JoinSet::new();
    for id in &instances {
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let ch = ch.clone();
        let id = id.clone();
        let db_type = db_type.clone();
        let metrics = metrics.clone();
        join.spawn(async move {
            let r = crate::db::query_db_metrics(&ch, &db_type, &id, start_ts, end_ts, &metrics).await;
            drop(permit);
            (id, r)
        });
    }

    let mut collected: std::collections::HashMap<String, Vec<MetricValue>> = Default::default();
    let mut app_err: std::collections::HashMap<String, String> = Default::default();
    while let Some(res) = join.join_next().await {
        match res {
            Ok((id, Ok(vals))) => {
                collected.insert(
                    id.clone(),
                    vals.into_iter()
                        .map(|v| MetricValue { name: v.name, value: v.value })
                        .collect(),
                );
            }
            Ok((id, Err(err))) => {
                app_err.insert(id, err);
            }
            Err(e) => return Err(format!("任务异常: {}", e)),
        }
    }

    let mut out = Vec::new();
    for id in &instances {
        let display = match name_map.get(id) {
            Some(n) if !n.is_empty() => format!("{}（{}）", id, n),
            _ => id.clone(),
        };
        out.push(AppMetrics {
            name: display,
            values: collected.remove(id).unwrap_or_default(),
            error: app_err.get(id).cloned(),
        });
    }
    Ok(out)
}

// ---------- 容器服务（TKE） ----------

#[tauri::command]
pub async fn list_clusters(config: Config) -> Result<Vec<crate::container::ClusterInfo>, String> {
    let ch = Channel::from_config(&config)?;
    crate::container::list_clusters(&ch).await
}

#[tauri::command]
pub async fn list_namespaces(config: Config, cluster_id: String) -> Result<Vec<String>, String> {
    let ch = Channel::from_config(&config)?;
    crate::container::list_namespaces(&ch, &cluster_id).await
}

#[tauri::command]
pub async fn list_deployments(
    config: Config,
    cluster_id: String,
    namespace: String,
) -> Result<Vec<crate::container::DeploymentInfo>, String> {
    let ch = Channel::from_config(&config)?;
    crate::container::list_deployments(&ch, &cluster_id, &namespace).await
}

/// 解析 k8s 资源量为数值：CPU→核（"500m"→0.5），内存→MiB（"6Gi"→6144）
fn parse_cpu(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(v) = s.strip_suffix('m') {
        return v.parse::<f64>().ok().map(|x| x / 1000.0);
    }
    s.parse::<f64>().ok()
}

fn parse_mem_mib(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(s.len()));
    let n: f64 = num.parse().ok()?;
    let mib = match unit {
        "Ki" => n / 1024.0,
        "Mi" => n,
        "Gi" => n * 1024.0,
        "Ti" => n * 1024.0 * 1024.0,
        "K" | "k" => n / 1024.0,
        "M" => n,
        "G" => n * 1024.0,
        _ => n / (1024.0 * 1024.0), // 纯字节
    };
    Some(mib)
}

#[tauri::command]
pub async fn query_container_metrics(
    config: Config,
    cluster_id: String,
    namespace: String,
    deployments: Vec<String>,
    start_ts: i64,
    end_ts: i64,
    apm_metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {
    if deployments.is_empty() {
        return Err("未选择工作负载".into());
    }
    let ch = Arc::new(Channel::from_config(&config)?);
    // 该命名空间的工作负载信息（SW_AGENT_NAME / limit / 副本）
    let all = crate::container::list_deployments(&ch, &cluster_id, &namespace).await?;
    let info_map: std::collections::HashMap<String, crate::container::DeploymentInfo> =
        all.into_iter().map(|d| (d.name.clone(), d)).collect();

    // APM 指标按视图分组（computed / instance_metric 单独处理）
    let mut apm_by_view: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut apm_instance: Vec<String> = Vec::new();
    for m in &apm_metrics {
        if m.view == "computed" {
            continue;
        }
        if m.view == "instance_metric" {
            apm_instance.push(m.name.clone());
        } else {
            apm_by_view.entry(m.view.clone()).or_default().push(m.name.clone());
        }
    }

    let sem = Arc::new(tokio::sync::Semaphore::new(5));
    let mut join = tokio::task::JoinSet::new();
    for name in &deployments {
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let ch = ch.clone();
        let name = name.clone();
        let info = info_map.get(&name).cloned();
        let cluster_id = cluster_id.clone();
        let namespace = namespace.clone();
        let region = config.region.clone();
        let apm_by_view = apm_by_view.clone();
        let apm_instance = apm_instance.clone();
        let apm_instance_id = config.instance_id.clone();
        join.spawn(async move {
            let mut values: Vec<MetricValue> = Vec::new();
            let mut err: Option<String> = None;

            let info = match info {
                Some(i) => i,
                None => {
                    drop(permit);
                    return (
                        name,
                        AppMetrics { name: String::new(), values, error: Some("工作负载不存在".into()) },
                    );
                }
            };

            // 1) 容器指标
            let (pods, ready) = match crate::container::deployment_pods(&ch, &cluster_id, &namespace, &name).await {
                Ok(v) => v,
                Err(e) => {
                    err = Some(format!("Pod 查询失败: {}", e));
                    (Vec::new(), 0)
                }
            };
            values.push(MetricValue { name: "pod_ready".into(), value: ready as f64 });
            values.push(MetricValue { name: "pod_desired".into(), value: info.replicas as f64 });
            match crate::container::pod_util_metrics(&ch, &cluster_id, &pods, start_ts, end_ts, &region).await {
                Ok(list) => {
                    for (k, v) in list {
                        values.push(MetricValue { name: k, value: v });
                    }
                }
                Err(e) => {
                    if err.is_none() {
                        err = Some(format!("容器指标查询失败: {}", e));
                    }
                }
            }
            if let Some(c) = parse_cpu(&info.cpu_limit) {
                values.push(MetricValue { name: "cpu_limit_cores".into(), value: c });
            }
            if let Some(m) = parse_mem_mib(&info.mem_limit) {
                values.push(MetricValue { name: "mem_limit_mib".into(), value: m });
            }

            // 2) APM 指标（SW_AGENT_NAME 严格匹配）
            if info.apm_name.is_empty() {
                if err.is_none() {
                    err = Some("未配置 SW_AGENT_NAME，无法关联 APM 应用".into());
                }
            } else if !apm_by_view.is_empty() || !apm_instance.is_empty() {
                for (view, names) in &apm_by_view {
                    match apm::fetch_app_metrics(&ch, &apm_instance_id, &info.apm_name, view, names, start_ts, end_ts).await {
                        Ok(vals) => {
                            for (k, v) in vals {
                                values.push(MetricValue { name: k, value: v });
                            }
                        }
                        Err(e) => {
                            if err.is_none() {
                                err = Some(format!("APM 指标查询失败: {}", e));
                            }
                        }
                    }
                }
                if !apm_instance.is_empty() {
                    match apm::fetch_instance_tops(&ch, &apm_instance_id, &info.apm_name, start_ts, end_ts, &apm_instance).await {
                        Ok(vals) => {
                            for (k, v) in vals {
                                values.push(MetricValue { name: k, value: v });
                            }
                        }
                        Err(e) => {
                            if err.is_none() {
                                err = Some(format!("APM 实例指标查询失败: {}", e));
                            }
                        }
                    }
                }
            }

            drop(permit);
            let display = if info.apm_name.is_empty() {
                format!("{}（{}）", info.name, namespace)
            } else {
                format!("{}（{}）→ APM: {}", info.name, namespace, info.apm_name)
            };
            (
                name,
                AppMetrics { name: display, values, error: err },
            )
        });
    }

    let mut collected: std::collections::HashMap<String, AppMetrics> = Default::default();
    while let Some(res) = join.join_next().await {
        match res {
            Ok((name, v)) => {
                collected.insert(name, v);
            }
            Err(e) => return Err(format!("任务异常: {}", e)),
        }
    }
    let mut out = Vec::new();
    for name in &deployments {
        if let Some(v) = collected.remove(name) {
            out.push(v);
        }
    }
    Ok(out)
}
