// Tauri 命令层
use crate::apm::{self, Channel};
use crate::cred;
use crate::config::{Config, MetricDef};
use serde::Serialize;
use std::sync::Arc;

/// 命令层统一处理配置：用户配置用前端传来的（最新），会话字段一律取磁盘上的
/// （前端已经不持有 cookie/csrfCode，直接用它传来的会把空会话带进查询）。
fn with_session(config: Config) -> Config {
    Config::load().merge_user_config(config)
}

#[tauri::command]
pub fn load_config() -> Config {
    Config::load()
}

/// 保存用户配置。会话字段（cookie/csrfCode/获取时间）永远以磁盘上的为准，
/// 前端无法把它们写坏（曾经出现前端用空值覆盖掉可用会话的事故）。
#[tauri::command]
pub fn save_config(config: Config) -> Result<(), String> {
    let disk = Config::load();
    disk.merge_user_config(config).save()
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
    let config = with_session(config);
    reset_cancel();
    let _busy = BusyGuard::new();
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

// ---------- 数据库监控 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbInstanceDto {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub async fn list_db_instances(config: Config, db_type: String) -> Result<Vec<DbInstanceDto>, String> {
    let config = with_session(config);
    let ch = Channel::from_config(&config)?;
    let list = crate::db::list_db_instances(&ch, &db_type).await?;
    Ok(list
        .into_iter()
        .map(|d| DbInstanceDto { id: d.id, name: d.name })
        .collect())
}

/// 把取到的指标压成一行，用于日志里回答"到底取到了什么"。
/// 最多列 12 项，避免一行日志刷屏。
fn fmt_metrics(vals: &[(String, f64)]) -> String {
    let mut s = vals
        .iter()
        .take(12)
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join(", ");
    if vals.len() > 12 {
        s.push_str(&format!(" …（共 {} 项）", vals.len()));
    }
    if s.is_empty() {
        s.push_str("(无数据)");
    }
    s
}

#[tauri::command]
pub async fn query_db_metrics(
    app: tauri::AppHandle,
    config: Config,
    db_type: String,
    start_ts: i64,
    end_ts: i64,
    instances: Vec<String>,
    metrics: Vec<String>,
) -> Result<Vec<AppMetrics>, String> {
    let config = with_session(config);
    reset_cancel();
    let _busy = BusyGuard::new();
    if instances.is_empty() {
        return Err("未选择数据库实例".into());
    }
    if metrics.is_empty() {
        return Err("未选择指标".into());
    }
    log_to(
        &app,
        format!(
            "开始查询数据库指标：{}，{} 个实例，每个 {} 项指标",
            db_type,
            instances.len(),
            metrics.len()
        ),
    );
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
                // 逐实例把取到的东西打出来：以前这里什么都不打，排查时看不出到底有没有取到
                let pairs: Vec<(String, f64)> =
                    vals.iter().map(|v| (v.name.clone(), v.value)).collect();
                let display = name_map.get(&id).map(|n| format!("{}（{}）", id, n)).unwrap_or_else(|| id.clone());
                log_to(&app, format!("  {}: 取到 {} 项 —— {}", display, vals.len(), fmt_metrics(&pairs)));
                collected.insert(
                    id.clone(),
                    vals.into_iter()
                        .map(|v| MetricValue { name: v.name, value: v.value })
                        .collect(),
                );
            }
            Ok((id, Err(err))) => {
                let display = name_map.get(&id).map(|n| format!("{}（{}）", id, n)).unwrap_or_else(|| id.clone());
                log_to(&app, format!("  {}: 查询失败 —— {}", display, cred::strip(&err)));
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
    log_to(
        &app,
        format!(
            "数据库指标查询完成：成功 {} / 共 {} 个实例",
            out.iter().filter(|a| a.error.is_none()).count(),
            out.len()
        ),
    );
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

/// 日志落盘路径：exe 同目录的 `apm-monitor.log`（便携版约定，和 config.json 放一起）。
/// 超过 1MB 就删掉重来，避免无限增长。
fn log_file_path() -> std::path::PathBuf {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);
    let p = dir.join("apm-monitor.log");
    if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > 1_000_000 {
        let _ = std::fs::remove_file(&p);
    }
    p
}

/// 把一行日志写进文件。
///
/// 为什么需要：界面日志面板有两个"看不到"的场景 ——
/// ① WebView2 数据目录坏了导致界面全白（见 HANDOFF 坑九），面板压根渲染不出来；
/// ② 查询中断/关闭程序后想事后复盘。
/// 落一份文件，排查时直接把文件发过来就行。exe 目录不可写时退到 %TEMP%。
pub fn log_to_file(msg: &str) {
    use std::io::Write;
    let line = format!(
        "[{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        msg
    );
    let write = |p: std::path::PathBuf| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .is_ok()
    };
    if !write(log_file_path()) {
        let _ = write(std::env::temp_dir().join("apm-monitor.log"));
    }
}

/// 前端日志面板里**由前端自己产生**的那些行（`开始统计…`、`统计完成：N 个对象…`、
/// `统计中断：需要重新登录…` 等）也落一份盘。
///
/// 为什么需要：`log_to` 只覆盖后端产生的行，前端那几行原先只在界面面板里 ——
/// 事后翻 `dist/apm-monitor.log` 会看不到"统计完成/中断"，排查时很容易误判成"卡住了"。
/// 后端自己的日志走 `log_to`，两处不重叠，所以不会重复。
#[tauri::command]
pub fn log_ui(msg: String) {
    log_to_file(&msg);
}

/// 输出一条进度日志到前端，并同步落盘
fn log_to(app: &tauri::AppHandle, msg: impl Into<String>) {
    use tauri::Emitter;
    let msg = cred::strip(&msg.into());
    log_to_file(&msg);
    let _ = app.emit("query-log", msg);
}

/// 登录窗口 label 前缀；每次用唯一后缀，避免 build 失败后 label 被永久占用
const LOGIN_LABEL_PREFIX: &str = "cloud-login";

/// 同一时刻只允许一个登录流程：否则「扫码登录」按钮会把自动刷新刚开的窗口顶掉
static LOGIN_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 是否有统计在跑（后台预刷新据此让路）
static QUERY_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 统计期间置位，函数返回（含提前 return）自动复位
struct BusyGuard;
impl BusyGuard {
    fn new() -> Self {
        QUERY_IN_FLIGHT.store(true, Ordering::SeqCst);
        BusyGuard
    }
}
impl Drop for BusyGuard {
    fn drop(&mut self) {
        QUERY_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
}

/// 会话失效后由前端调用：中断后端在跑的请求 + 拉起登录窗口（不等扫码）
#[tauri::command]
pub async fn notify_session_invalid(app: tauri::AppHandle) {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
    ensure_login_window(&app).await;
}

// ---------- 容器服务（TKE） ----------

/// 控制台专属能力（K8s 资源列表/平台转发）只有 Cookie 通道有；官方 API 无对应接口
fn console_channel(config: &Config) -> Result<Channel, String> {
    Channel::from_config(config)
}


#[tauri::command]
pub async fn list_clusters(config: Config) -> Result<Vec<crate::container::ClusterInfo>, String> {
    let ch = console_channel(&config)?;
    crate::container::list_clusters(&ch).await
}

#[tauri::command]
pub async fn list_namespaces(config: Config, cluster_id: String) -> Result<Vec<String>, String> {
    let config = with_session(config);
    let ch = console_channel(&config)?;
    crate::container::list_namespaces(&ch, &cluster_id).await
}

#[tauri::command]
pub async fn list_deployments(
    config: Config,
    cluster_id: String,
    namespace: String,
) -> Result<Vec<crate::container::DeploymentInfo>, String> {
    let config = with_session(config);
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
    // 该命名空间的工作负载信息（SW_AGENT_NAME / limit / 副本）
    log(format!("读取命名空间 {} 的工作负载列表…", namespace));
    let all = crate::container::list_deployments(&ch, &cluster_id, &namespace).await?;
    log(format!("共 {} 个工作负载，开始逐个查询", all.len()));
    let info_map: std::collections::HashMap<String, crate::container::DeploymentInfo> =
        all.into_iter().map(|d| (d.name.clone(), d)).collect();

    // APM 指标按视图分组（computed / instance_metric 单独处理）
    // 视图名必须是我们认识的那几个：旧版配置里可能残留 `container`/`mysql`/`redis`/`mongodb`
    // 这类**不存在的视图名**，直接发出去每个工作负载都会白打一串
    // `FailedOperation.ViewNameNotExistOrIllegal`。这里跳过并说明，别浪费请求。
    const KNOWN_APM_VIEWS: [&str; 4] = ["service_metric", "sql_metric", "mq_metric", "runtime_metric"];
    let mut apm_by_view: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut apm_instance: Vec<String> = Vec::new();
    for m in &apm_metrics {
        if m.view == "computed" {
            continue;
        }
        if m.view == "instance_metric" {
            apm_instance.push(m.name.clone());
        } else if KNOWN_APM_VIEWS.contains(&m.view.as_str()) {
            apm_by_view.entry(m.view.clone()).or_default().push(m.name.clone());
        } else {
            log(format!(
                "跳过指标 {}（视图名 \"{}\" 不是 APM 的视图，可能是旧配置残留）",
                m.name, m.view
            ));
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
            // 旧网关走配置自助（csrf=bkn(skey)、配置 Cookie，见 apm::call_capi），
            // 不需要预热窗口/浏览器会话；被网关拒时才由 pod_util_metrics 内部走页面退路。
            let util = crate::container::pod_util_metrics(
                &ch, app_inner.as_ref(), &cluster_id, &namespace, &name, &pods,
                start_ts, end_ts, &region,
            )
            .await;
            match util {
                Ok(list) => {
                    if let Some(a) = &app_inner {
                        log_to(a, format!("  {}: 容器利用率取到 {} 项 —— {}", name, list.len(), fmt_metrics(&list)));
                    }
                    for (k, v) in list {
                        values.push(MetricValue { name: k, value: v });
                    }
                }
                Err(e) => {
                    if err.is_none() {
                        err = Some(format!("容器指标查询失败: {}", e));
                    }
                    // 也写一条进度日志：容器这行只在结果区显示，落盘后方便事后排查
                    if let Some(a) = &app_inner {
                        log_to(a, format!("{}: 容器指标查询失败: {}", name, e));
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
                            // 把取到的 APM 指标打出来（以前只打"正在查询"，看不出到底有没有取到）
                            if let Some(a) = &app_inner {
                                log_to(a, format!("  {}: APM[{}] 取到 {} 项 —— {}", name, view, vals.len(), fmt_metrics(&vals)));
                            }
                            for (k, v) in vals {
                                values.push(MetricValue { name: k, value: v });
                            }
                        }
                        Err(e) => {
                            if let Some(a) = &app_inner {
                                log_to(a, format!("  {}: APM[{}] 查询失败 —— {}", name, view, cred::strip(&e)));
                            }
                            if err.is_none() {
                                err = Some(format!("APM 指标查询失败: {}", e));
                            }
                        }
                    }
                }
                if !apm_instance.is_empty() {
                    match apm::fetch_instance_tops(&ch, &apm_instance_id, &info.apm_name, start_ts, end_ts, &apm_instance).await {
                        Ok(vals) => {
                            if let Some(a) = &app_inner {
                                log_to(a, format!("  {}: APM 实例指标取到 {} 项 —— {}", name, vals.len(), fmt_metrics(&vals)));
                            }
                            for (k, v) in vals {
                                values.push(MetricValue { name: k, value: v });
                            }
                        }
                        Err(e) => {
                            if let Some(a) = &app_inner {
                                log_to(a, format!("  {}: APM 实例指标查询失败 —— {}", name, cred::strip(&e)));
                            }
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

// ---------- 登录 / 会话刷新 ----------

/// 一次登录抓取：开窗 → 等会话 → 校验 → 落盘（校验不过就不动原会话）。
/// visible=false 的隐藏窗口 + silent=true 只记失败日志，供后台预刷新使用。
async fn run_login_capture(
    app: &tauri::AppHandle,
    timeout: std::time::Duration,
    visible: bool,
    silent: bool,
) -> Result<(), String> {
    if LOGIN_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有一个登录/刷新流程在进行中".into());
    }
    let res = capture_once(app, timeout, visible, silent).await;
    LOGIN_IN_FLIGHT.store(false, Ordering::SeqCst);
    res
}

async fn capture_once(
    app: &tauri::AppHandle,
    timeout: std::time::Duration,
    visible: bool,
    silent: bool,
) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    // 清掉上一次残留的登录窗口（label 带唯一后缀，所以按前缀扫）
    for (label, w) in app.webview_windows() {
        if label.starts_with(LOGIN_LABEL_PREFIX) {
            let _ = w.destroy();
        }
    }
    let label = format!("{}-{}", LOGIN_LABEL_PREFIX, chrono::Utc::now().timestamp_millis());
    let url = crate::login::LOGIN_URL
        .parse()
        .map_err(|e| format!("登录地址无效: {}", e))?;
    if !silent {
        log_to(app, "正在打开登录窗口…");
    }
    // additional_browser_args 必须与主窗口（tauri.conf.json）一致，否则 WebView2 建不出这个 webview；
    // focused(false)：显示但不抢焦点
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("扫码登录腾讯云（登录后自动关闭）")
        .inner_size(1080.0, 760.0)
        .focused(false)
        .additional_browser_args(crate::login::BROWSER_ARGS);
    builder = if visible {
        builder.always_on_top(true)
    } else {
        builder.visible(false).skip_taskbar(true)
    };
    if let Err(e) = builder.build() {
        if !silent {
            log_to(app, format!("打开登录窗口失败：{}", e));
        }
        return Err(format!("打开登录窗口失败: {}", e));
    }
    if !silent {
        if visible {
            log_to(
                app,
                format!(
                    "登录窗口已打开，请在窗口里扫码（最多 {} 秒）；扫完请重新点「开始统计」",
                    timeout.as_secs()
                ),
            );
        } else {
            log_to(app, "正在后台刷新会话…");
        }
    }

    // 快捷登录只在「手动打开的可见登录窗口」里自动点；后台隐藏刷新窗口绝不点（静默、无人看着）。
    let use_quick = !silent && crate::config::Config::load().session_auto_quick_login;
    let res = crate::login::wait_for_login(timeout, use_quick).await;
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }

    match res {
        Ok(r) if crate::login::cookie_has_uin(&r.cookie) => {
            match validate_login_session(&r.cookie, &r.csrf_code).await {
                Ok(()) => {
                    let mut cfg = crate::config::Config::load();
                    let kept = cfg.apply_login(r.cookie.clone(), r.csrf_code.clone());
                    cfg.save()?;
                    if !silent {
                        if r.auto_quick_clicked {
                            log_to(app, "已自动点击「微信快捷登录」，本次免扫码");
                        }
                        if !kept.is_empty() {
                            log_to(app, format!("以下字段未更新，沿用原值：{}", kept.join(", ")));
                        }
                        log_to(
                            app,
                            format!(
                                "会话已更新（cookie {} 字符，uin={} ownerUin={}）",
                                r.cookie.len(),
                                if r.uin.is_empty() { "-" } else { &r.uin },
                                if r.owner_uin.is_empty() { "-" } else { &r.owner_uin }
                            ),
                        );
                    }
                    Ok(())
                }
                Err(e) => {
                    // 打一行 Cookie 完整度：这是定位"偶发半套 Cookie"的唯一线索
                    let desc = crate::login::describe_cookie(&r.cookie);
                    if !silent {
                        log_to(app, format!("新会话未通过校验（{}），已保留原会话", crate::cred::strip(&e)));
                        log_to(app, format!("  抓到的 Cookie 完整度：{}", desc));
                    }
                    Err(format!(
                        "新会话未通过校验，已保留原会话：{}（Cookie 完整度：{}）",
                        crate::cred::strip(&e),
                        desc
                    ))
                }
            }
        }
        Ok(r) => {
            if !silent {
                log_to(app, "登录窗口里没有检测到登录态（Cookie 缺 uin），本次不覆盖已有会话");
                if r.auto_quick_clicked {
                    log_to(app, "已点击「微信快捷登录」，但未检测到会话（可能需要手机确认），请扫码或检查窗口");
                }
                log_to(app, format!("  抓到的 Cookie 完整度：{}", crate::login::describe_cookie(&r.cookie)));
            }
            Err("未检测到登录态".into())
        }
        Err(e) => {
            if !silent {
                log_to(app, format!("等待登录失败：{}", crate::cred::strip(&e)));
            }
            Err(e)
        }
    }
}

/// 用户手动点「扫码登录」：等扫码完成后返回（前端显示结果）
#[tauri::command]
pub async fn start_cloud_login(app: tauri::AppHandle) -> Result<(), String> {
    run_login_capture(&app, std::time::Duration::from_secs(600), true, false).await
}

/// 统计中判定为"会话失效"时调用：先等正在进行的登录/刷新（它若把会话救回来就不开窗），
/// 否则开一个可见窗口并后台抓取——不等扫码，扫完由用户重新点统计。
async fn ensure_login_window(app: &tauri::AppHandle) {
    let before = crate::config::Config::load().session_fetched_at;
    for _ in 0..80 {
        if !LOGIN_IN_FLIGHT.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let cfg = crate::config::Config::load();
    if cfg.session_ready() && cfg.session_fetched_at > before {
        log_to(app, "会话已更新，请重新点「开始统计」");
        return;
    }
    log_to(app, "需要重新登录：已打开登录窗口，请扫码；扫完请重新点「开始统计」");
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = run_login_capture(&a, std::time::Duration::from_secs(600), true, false).await;
    });
}

/// 后台会话维护：空闲时按阈值静默刷新（隐藏窗口，成功不写日志）
pub fn spawn_session_keeper(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut fails: i64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            let cfg = crate::config::Config::load();
            if !cfg.session_auto_refresh || !cfg.session_ready() || QUERY_IN_FLIGHT.load(Ordering::SeqCst) {
                continue;
            }
            if cfg.session_age_secs() < cfg.session_refresh_minutes.max(1) * 60 {
                continue;
            }
            let max_fails = cfg.session_refresh_max_fails.max(1);
            if fails >= max_fails {
                continue;
            }
            let before = cfg.session_fetched_at;
            match run_login_capture(&app, std::time::Duration::from_secs(40), false, true).await {
                Ok(()) => fails = 0,
                Err(e) => {
                    fails += 1;
                    log_to(
                        &app,
                        format!(
                            "后台刷新会话失败（第 {}/{} 次）：{}",
                            fails,
                            max_fails,
                            crate::cred::strip(&e)
                        ),
                    );
                    if crate::config::Config::load().session_fetched_at == before {
                        tokio::time::sleep(std::time::Duration::from_secs(
                            (cfg.session_refresh_backoff_minutes.max(1) * 60) as u64,
                        ))
                        .await;
                    }
                }
            }
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub ready: bool,
    pub uin: String,
    pub fetched_at: i64,
    pub age_secs: i64,
    pub auto_refresh: bool,
    pub refresh_minutes: i64,
    pub backoff_minutes: i64,
    pub max_fails: i64,
    pub auto_quick_login: bool,
}

/// 设置界面用的会话状态
#[tauri::command]
pub fn session_status() -> SessionStatus {
    let c = crate::config::Config::load();
    let (uin, _) = c.extract_ids_from_cookie();
    SessionStatus {
        ready: c.session_ready(),
        uin,
        fetched_at: c.session_fetched_at,
        age_secs: c.session_age_secs(),
        auto_refresh: c.session_auto_refresh,
        refresh_minutes: c.session_refresh_minutes,
        backoff_minutes: c.session_refresh_backoff_minutes,
        max_fails: c.session_refresh_max_fails,
        auto_quick_login: c.session_auto_quick_login,
    }
}

/// 用新会话做一次轻量控制台调用，确认服务端认它
async fn validate_login_session(cookie: &str, csrf_code: &str) -> Result<(), String> {
    let mut cfg = crate::config::Config::load();
    cfg.cookie = cookie.to_string();
    cfg.csrf_code = csrf_code.to_string();
    let ch = Channel::from_config(&cfg)?;
    crate::container::list_clusters(&ch).await.map(|_| ())
}

/// 容器统计（资源列表 + 容器利用率 + 关联 APM 指标）
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
    let config = with_session(config);
    reset_cancel();
    let _busy = BusyGuard::new();
    // 前端已经打过一条"开始统计…"，这里只补充这次统计的范围，不再重复那一句
    log_to(
        &app,
        format!(
            "容器统计范围：集群 {} / 命名空间 {}，{} 个工作负载，{} 项 APM 指标",
            if cluster_id.is_empty() { "-" } else { &cluster_id },
            if namespace.is_empty() { "-" } else { &namespace },
            deployments.len(),
            apm_metrics.len()
        ),
    );
    container_query(
        Some(app.clone()),
        config,
        cluster_id,
        namespace,
        deployments,
        start_ts,
        end_ts,
        apm_metrics,
    )
    .await
}
