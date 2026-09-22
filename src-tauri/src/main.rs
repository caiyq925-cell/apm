#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apm;
mod commands;
mod config;
mod console;
mod container;
mod db;
mod login;
mod reqable;
mod tc3;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::save_config,
            commands::list_apps,
            commands::refresh_metric_defs,
            commands::query_metrics,
            commands::validate_cookie,
            commands::list_db_instances,
            commands::query_db_metrics,
            commands::list_clusters,
            commands::list_namespaces,
            commands::list_deployments,
            commands::query_container_metrics,
            commands::cancel_query,
            commands::start_cloud_login,
            commands::fetch_login_from_reqable
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
