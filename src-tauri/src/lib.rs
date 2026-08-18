mod commands;
mod secrets;
mod state;

use std::sync::Arc;

use tauri::Manager;

use dby_core::config::AppConfig;
use dby_core::history::HistoryStore;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let config_path = data_dir.join("config.json");
            let history_path = data_dir.join("history.db");

            let config =
                AppConfig::load(&config_path).unwrap_or_else(|_| AppConfig::with_default_project());
            let history = HistoryStore::open(&history_path)
                .expect("failed to open SQL history store (SQLite/FTS5)");

            app.manage(Arc::new(AppState::new(config, history, config_path)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_drivers,
            commands::test_connection,
            commands::probe_host_key,
            commands::connect,
            commands::disconnect,
            commands::list_connections,
            commands::list_saved_connections,
            commands::reconnect,
            commands::delete_saved_connection,
            commands::update_saved_connection,
            commands::list_databases,
            commands::list_tables,
            commands::list_columns,
            commands::execute_query,
            commands::execute_query_stream,
            commands::cancel_query,
            commands::analyze_danger,
            commands::begin,
            commands::commit,
            commands::rollback,
            commands::set_autocommit,
            commands::export_result,
            commands::build_edit_sql,
            commands::execute_edit,
            commands::create_database,
            commands::drop_database,
            commands::create_table,
            commands::rename_table,
            commands::drop_table,
            commands::list_projects,
            commands::create_project,
            commands::rename_project,
            commands::delete_project,
            commands::search_history,
            commands::list_history,
            commands::list_executions,
            commands::pin_statement,
            commands::delete_execution,
        ])
        .run(tauri::generate_context!())
        .expect("error while running dby");
}
