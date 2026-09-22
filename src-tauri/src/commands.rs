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
