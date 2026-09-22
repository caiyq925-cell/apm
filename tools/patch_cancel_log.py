import io

# ---------------- 后端 ----------------
p = "src-tauri/src/commands.rs"
s = io.open(p, encoding="utf-8").read()

# 1) 取消标志 + 进度日志辅助 + cancel 命令
helpers = '''
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

'''
if "CANCEL_FLAG" not in s:
    s = s.replace("// ---------- 容器服务（TKE） ----------", helpers + "// ---------- 容器服务（TKE） ----------", 1)

# 2) container_query 增加 app 参数（用于日志）——改为内部再包一层
s = s.replace('''async fn container_query(
    config: Config,
    cluster_id: String,
    namespace: String,
    deployments: Vec<String>,
    start_ts: i64,
    end_ts: i64,
    apm_metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {''','''async fn container_query(
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
    };''', 1)

# 3) 工作负载列表前后日志 + 取消检查
s = s.replace('''    // 该命名空间的工作负载信息（SW_AGENT_NAME / limit / 副本）
    let all = crate::container::list_deployments(&ch, &cluster_id, &namespace).await?;''',
'''    // 该命名空间的工作负载信息（SW_AGENT_NAME / limit / 副本）
    log(format!("读取命名空间 {} 的工作负载列表…", namespace));
    let all = crate::container::list_deployments(&ch, &cluster_id, &namespace).await?;
    log(format!("共 {} 个工作负载，开始逐个查询", all.len()));''', 1)

# 4) 任务内日志与取消（在 spawn 前检查取消）
s = s.replace('''    for name in &deployments {
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;''',
'''    for name in &deployments {
        if is_cancelled() {
            log("已停止：中断剩余工作负载查询".into());
            break;
        }
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;''', 1)

# 5) 单个工作负载内部日志
s = s.replace('''            // 1) 容器指标
            let (pods, ready) = match crate::container::deployment_pods(&ch, &cluster_id, &namespace, &name).await {''',
'''            // 1) 容器指标
            if let Some(a) = &app_inner {
                log_to(a, format!("{}: 读取 Pod 列表…", name));
            }
            let (pods, ready) = match crate::container::deployment_pods(&ch, &cluster_id, &namespace, &name).await {''', 1)
s = s.replace('''            let fb = fallback_ch.as_deref();
            match crate::container::pod_util_metrics(&ch, fb, &cluster_id, &pods, start_ts, end_ts, &region).await {''',
'''            if let Some(a) = &app_inner {
                log_to(a, format!("{}: 查询容器 CPU/内存利用率（{} 个 Pod）…", name, pods.len()));
            }
            let fb = fallback_ch.as_deref();
            match crate::container::pod_util_metrics(&ch, fb, &cluster_id, &pods, start_ts, end_ts, &region).await {''', 1)
s = s.replace('''            // 2) APM 指标（SW_AGENT_NAME 严格匹配）''',
'''            if let Some(a) = &app_inner {
                log_to(a, format!("{}: 查询 APM 指标（SW_AGENT_NAME={}）…", name, if info.apm_name.is_empty() { "-" } else { &info.apm_name }));
            }
            // 2) APM 指标（SW_AGENT_NAME 严格匹配）''', 1)
# 任务闭包内可用的 app 句柄
s = s.replace('''        let fallback_ch = fallback_ch.clone();
        join.spawn(async move {''','''        let fallback_ch = fallback_ch.clone();
        let app_inner = app.clone();
        join.spawn(async move {''', 1)

# 6) 命令包装：重置取消标志、日志、刷新会话时提示与更短等待
s = s.replace('''    let mut cfg = config.clone();
    let first = container_query(
        cfg.clone(),''','''    reset_cancel();
    log_to(&app, "开始统计…");
    let mut cfg = config.clone();
    let first = container_query(
        Some(app.clone()),
        cfg.clone(),''', 1)
s = s.replace('''    let second = container_query(
        cfg,''','''    log_to(&app, "会话已刷新，重新统计…");
    let second = container_query(
        Some(app.clone()),
        cfg,''', 1)
s = s.replace('''    {
        use tauri::Emitter;
        let _ = app.emit("session-refresh", "控制台会话已过期，正在刷新…");
    }''','''    log_to(&app, "控制台会话已过期，正在打开登录窗口刷新会话（如未登录请扫码）…");
    {
        use tauri::Emitter;
        let _ = app.emit("session-refresh", "控制台会话已过期，正在刷新…");
    }''', 1)

# 7) 其他查询类型也重置取消标志 + 支持中断
for fn in ["pub async fn query_metrics(", "pub async fn query_db_metrics("]:
    idx = s.find(fn)
    if idx != -1:
        brace = s.find("{", idx)
        s = s[:brace + 1] + "\n    reset_cancel();" + s[brace + 1:]

io.open(p, "w", encoding="utf-8").write(s)
print("commands ok")

# 8) main.rs 注册 cancel_query
p = "src-tauri/src/main.rs"
s = io.open(p, encoding="utf-8").read()
if "commands::cancel_query" not in s:
    s = s.replace("            commands::start_cloud_login,",
                  "            commands::cancel_query,\n            commands::start_cloud_login,", 1)
    io.open(p, "w", encoding="utf-8").write(s)
    print("main ok")

# 9) 登录等待超时 300s → 120s
p = "src-tauri/src/login.rs"
s = io.open(p, encoding="utf-8").read()
s = s.replace("let deadline = tokio::time::Instant::now() + Duration::from_secs(300);",
              "let deadline = tokio::time::Instant::now() + Duration::from_secs(120);")
io.open(p, "w", encoding="utf-8").write(s)
print("login ok")
