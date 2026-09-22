import io, re

p = "src-tauri/src/commands.rs"
s = io.open(p, encoding="utf-8").read()

# 1) 把原命令改名为内部函数（去掉 tauri 命令宏），后再包一层带自动刷新的命令
old_sig = '''#[tauri::command]
pub async fn query_container_metrics(
    config: Config,
    cluster_id: String,
    namespace: String,
    deployments: Vec<String>,
    start_ts: i64,
    end_ts: i64,
    apm_metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {'''
new_sig = '''async fn container_query(
    config: Config,
    cluster_id: String,
    namespace: String,
    deployments: Vec<String>,
    start_ts: i64,
    end_ts: i64,
    apm_metrics: Vec<MetricDef>,
) -> Result<Vec<AppMetrics>, String> {'''
assert old_sig in s, "命令签名未找到"
s = s.replace(old_sig, new_sig, 1)

# 2) 抽出「打开登录窗口并取回会话」的复用函数，并让 start_cloud_login 使用它
old_login = '''#[tauri::command]
pub async fn start_cloud_login(
    app: tauri::AppHandle,
    config: Config,
) -> Result<Config, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window("cloud-login") {
        let _ = w.close();
    }
    let url = crate::login::LOGIN_URL
        .parse()
        .map_err(|e| format!("登录地址无效: {}", e))?;
    let args = format!("--remote-debugging-port={}", crate::login::CDP_PORT);
    WebviewWindowBuilder::new(&app, "cloud-login", WebviewUrl::External(url))
        .title("登录腾讯云（微信扫码，登录后自动关闭）")
        .inner_size(1080.0, 760.0)
        .additional_browser_args(&args)
        .build()
        .map_err(|e| format!("打开登录窗口失败: {}", e))?;

    let res = crate::login::wait_for_login().await;
    if let Some(w) = app.get_webview_window("cloud-login") {
        let _ = w.close();
    }
    let res = res?;
    let mut c = config;
    c.cookie = res.cookie;
    c.uin = res.uin;
    c.owner_uin = res.owner_uin;
    c.csrf_code = res.csrf_code;
    c.auth_mode = "cookie".into();
    c.save()?;
    Ok(c)
}'''
new_login = '''/// 打开登录窗口 → 等待会话信息（登录态通常已存在，页面加载即可拿到新令牌）→ 关闭窗口
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
    let mut cfg = config.clone();
    let first = container_query(
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
    {
        use tauri::Emitter;
        let _ = app.emit("session-refresh", "控制台会话已过期，正在刷新…");
    }
    let res = match open_login_and_capture(&app).await {
        Ok(r) => r,
        Err(e) => return Ok(first), // 刷新失败则返回首次结果（含错误提示）
    };
    cfg.cookie = res.cookie;
    cfg.uin = res.uin;
    cfg.owner_uin = res.owner_uin;
    cfg.csrf_code = res.csrf_code;
    let _ = cfg.save();

    let second = container_query(
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
}'''
assert old_login in s, "start_cloud_login 片段未找到"
s = s.replace(old_login, new_login, 1)
io.open(p, "w", encoding="utf-8").write(s)
print("commands ok")
