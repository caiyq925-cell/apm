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
    reset_cancel();
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
    reset_cancel();
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


// ---------- 查询取消与进度日志 ----------
use std::sync::atomic::{AtomicBool, Ordering};
static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

fn reset_cancel() {
    CANCEL_FLAG.store(false, Ordering::SeqCst);
}
fn is_cancelled() -> bool {
    CANCEL_FLAG.load(Ordering::SeqCst)
}

/// 结束查询（前端「停止」按钮）
#[tauri::command]
pub fn cancel_query() {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
}

/// 输出一条进度日志到前端
fn log_to(app: &tauri::AppHandle, msg: impl Into<String>) {
    use tauri::Emitter;
    let _ = app.emit("query-log", msg.into());
}

// ---------- 容器服务（TKE） ----------

/// 控制台专属能力（K8s 资源列表/平台转发）只有 Cookie 通道有；官方 API 无对应接口
fn console_channel(config: &Config) -> Result<Channel, String> {
    if !config.cookie.trim().is_empty() {
        Ok(Channel::Cookie(config.clone()))
    } else {
        Err("请先在设置中完成登录（Cookie），工作负载与 Pod 列表需要控制台会话".into())
    }
}


#[tauri::command]
pub async fn list_clusters(config: Config) -> Result<Vec<crate::container::ClusterInfo>, String> {
    let ch = console_channel(&config)?;
    crate::container::list_clusters(&ch).await
}

#[tauri::command]
pub async fn list_namespaces(config: Config, cluster_id: String) -> Result<Vec<String>, String> {
    let ch = console_channel(&config)?;
    crate::container::list_namespaces(&ch, &cluster_id).await
}

#[tauri::command]
pub async fn list_deployments(
    config: Config,
    cluster_id: String,
    namespace: String,
) -> Result<Vec<crate::container::DeploymentInfo>, String> {
    let ch = console_channel(&config)?;
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

async fn container_query(
    app: Option<tauri::AppHandle>,
    config: Config,
    cluster_id: String,
    namespace: String,
    deployments: Vec<String>,
    start_ts: i64,
    end_ts: i64,
    apm_metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {
    let log = |m: String| {
        if let Some(a) = &app {
            log_to(a, m);
        }
    };
    if deployments.is_empty() {
        return Err("未选择工作负载".into());
    }
    // 资源列表（Deployment/Pod/SW_AGENT_NAME）走 Cookie 通道
    let ch = Arc::new(console_channel(&config)?);
    // 配置了密钥时，容器指标优先走官方 API（长期有效）
    let fallback_ch: Option<Arc<Channel>> = if !config.secret_id.is_empty() && !config.secret_key.is_empty() {
        Some(Arc::new(Channel::Secret {
            region: config.region.clone(),
            cred: crate::tc3::Tc3Credential {
                secret_id: config.secret_id.clone(),
                secret_key: config.secret_key.clone(),
            },
        }))
    } else {
        None
    };
    // 该命名空间的工作负载信息（SW_AGENT_NAME / limit / 副本）
    log(format!("读取命名空间 {} 的工作负载列表…", namespace));
    let all = crate::container::list_deployments(&ch, &cluster_id, &namespace).await?;
    log(format!("共 {} 个工作负载，开始逐个查询", all.len()));
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
        if is_cancelled() {
            log("已停止：中断剩余工作负载查询".into());
            break;
        }
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
        let fallback_ch = fallback_ch.clone();
        let app_inner = app.clone();
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
            if let Some(a) = &app_inner {
                log_to(a, format!("{}: 读取 Pod 列表…", name));
            }
            let (pods, ready) = match crate::container::deployment_pods(&ch, &cluster_id, &namespace, &name).await {
                Ok(v) => v,
                Err(e) => {
                    err = Some(format!("Pod 查询失败: {}", e));
                    (Vec::new(), 0)
                }
            };
            values.push(MetricValue { name: "pod_ready".into(), value: ready as f64 });
            values.push(MetricValue { name: "pod_desired".into(), value: info.replicas as f64 });
            if let Some(a) = &app_inner {
                log_to(a, format!("{}: 查询容器 CPU/内存利用率（{} 个 Pod）…", name, pods.len()));
            }
            let fb = fallback_ch.as_deref();
            match crate::container::pod_util_metrics(&ch, fb, &cluster_id, &pods, start_ts, end_ts, &region).await {
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

            if let Some(a) = &app_inner {
                log_to(a, format!("{}: 查询 APM 指标（SW_AGENT_NAME={}）…", name, if info.apm_name.is_empty() { "-" } else { &info.apm_name }));
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
            let display = info.name.clone();
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

// ---------- 登录（内嵌扫码窗口 / 从 Reqable 抓取） ----------

/// 打开腾讯云官方登录页窗口，用户扫码登录后自动取回会话信息并保存
/// 打开登录窗口 → 等待会话信息（登录态通常已存在，页面加载即可拿到新令牌）→ 关闭窗口
async fn open_login_and_capture(app: &tauri::AppHandle) -> Result<crate::login::LoginResult, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window("cloud-login") {
        let _ = w.close();
    }
    let url = crate::login::LOGIN_URL
        .parse()
        .map_err(|e| format!("登录地址无效: {}", e))?;
    let args = format!("--remote-debugging-port={}", crate::login::CDP_PORT);
    WebviewWindowBuilder::new(app, "cloud-login", WebviewUrl::External(url))
        .title("正在刷新腾讯云会话（登录后自动关闭）")
        .inner_size(1080.0, 760.0)
        .additional_browser_args(&args)
        .build()
        .map_err(|e| format!("打开登录窗口失败: {}", e))?;

    let res = crate::login::wait_for_login().await;
    if let Some(w) = app.get_webview_window("cloud-login") {
        let _ = w.close();
    }
    res
}

#[tauri::command]
pub async fn start_cloud_login(
    app: tauri::AppHandle,
    config: Config,
) -> Result<Config, String> {
    let res = open_login_and_capture(&app).await?;
    let mut c = config;
    c.cookie = res.cookie;
    c.uin = res.uin;
    c.owner_uin = res.owner_uin;
    c.csrf_code = res.csrf_code;
    c.auth_mode = "cookie".into();
    c.save()?;
    Ok(c)
}

/// 容器统计：会话过期（1216）时自动刷新会话并重试一次
#[tauri::command]
pub async fn query_container_metrics(
    app: tauri::AppHandle,
    config: Config,
    cluster_id: String,
    namespace: String,
    deployments: Vec<String>,
    start_ts: i64,
    end_ts: i64,
    apm_metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {
    reset_cancel();
    log_to(&app, "开始统计…");
    let mut cfg = config.clone();
    let first = container_query(
        Some(app.clone()),
        cfg.clone(),
        cluster_id.clone(),
        namespace.clone(),
        deployments.clone(),
        start_ts,
        end_ts,
        apm_metrics.clone(),
    )
    .await?;

    let need_refresh = first
        .iter()
        .any(|r| r.error.as_deref().unwrap_or("").contains("1216"));
    if !need_refresh {
        return Ok(first);
    }

    // 通知前端：正在刷新会话
    log_to(&app, "控制台会话已过期，正在打开登录窗口刷新会话（如未登录请扫码）…");
    {
        use tauri::Emitter;
        let _ = app.emit("session-refresh", "控制台会话已过期，正在刷新…");
    }
    let res = match open_login_and_capture(&app).await {
        Ok(r) => r,
        Err(_) => return Ok(first), // 刷新失败则返回首次结果（含错误提示）
    };
    cfg.cookie = res.cookie;
    cfg.uin = res.uin;
    cfg.owner_uin = res.owner_uin;
    cfg.csrf_code = res.csrf_code;
    let _ = cfg.save();

    log_to(&app, "会话已刷新，重新统计…");
    let second = container_query(
        Some(app.clone()),
        cfg,
        cluster_id,
        namespace,
        deployments,
        start_ts,
        end_ts,
        apm_metrics,
    )
    .await?;
    Ok(second)
}

/// 可选：从 Reqable 抓包提取登录信息（需要 Reqable 正在运行并抓过控制台请求）
#[tauri::command]
pub async fn fetch_login_from_reqable(config: Config) -> Result<Config, String> {
    let info = crate::reqable::fetch_login_info("", 9000).await?;
    let mut c = config;
    c.cookie = info.cookie;
    c.uin = info.uin;
    c.owner_uin = info.owner_uin;
    c.csrf_code = info.csrf_code;
    c.auth_mode = "cookie".into();
    c.save()?;
    Ok(c)
}
