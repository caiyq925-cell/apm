#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apm;
mod capi;
mod cred;
mod commands;
mod config;
mod console;
mod container;
mod db;
mod login;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::save_config,
            commands::list_apps,
            commands::refresh_metric_defs,
            commands::query_metrics,
            commands::list_db_instances,
            commands::query_db_metrics,
            commands::list_clusters,
            commands::list_namespaces,
            commands::list_deployments,
            commands::query_container_metrics,
            commands::cancel_query,
            commands::log_ui,
            commands::start_cloud_login,
            commands::session_status,
            commands::notify_session_invalid
        ])
        .setup(|app| {
            // 启动标记落盘：界面白屏（WebView2 数据目录坏了）时，这是唯一能证明
            // "程序确实起来了、跑的是哪个 exe"的证据。见 HANDOFF 坑九。
            commands::log_to_file(&format!(
                "=== 应用启动 v{} | exe: {}",
                env!("CARGO_PKG_VERSION"),
                std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "(未知)".into())
            ));
            commands::spawn_session_keeper(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
