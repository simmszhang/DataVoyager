mod commands;
mod secrets;
mod state;

use std::sync::Arc;

use tauri::Manager;

use dby_core::config::AppConfig;
use dby_core::error::Result;
use dby_core::history::HistoryStore;

use secrets::{secret_key, set_secret, SecretKind};
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let config_path = data_dir.join("config.json");
            let history_path = data_dir.join("history.db");

            let mut config =
                AppConfig::load(&config_path).unwrap_or_else(|_| AppConfig::with_default_project());
            // 迁移旧版明文 SSH 凭据进钥匙串并清除落盘（#22）；单条失败不阻断启动
            if migrate_legacy_secrets(&mut config) > 0 {
                if let Err(e) = config.save(&config_path) {
                    log::warn!("迁移后重写 config.json 失败（明文可能仍残留磁盘）: {e}");
                }
            }
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
            commands::build_table_select,
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
            commands::build_insert_sql,
            commands::show_create_table,
            commands::execute_edit,
            commands::create_database,
            commands::drop_database,
            commands::create_table,
            commands::rename_table,
            commands::drop_table,
            commands::drop_view,
            commands::drop_routine,
            commands::drop_trigger,
            commands::truncate_table,
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

/// 迁移旧版 `config.json` 中的明文 SSH 凭据进钥匙串并清除（#22）。
/// 每条连接：**先写 keyring、成功后清空字段**；写失败**不**清除（保留明文，下次重试），
/// `log::warn!` 跳过该字段、继续其余，不中断启动。返回清空了至少一个字段的连接数。
/// 幂等：二次运行时 `password/private_key` 已为 `None` → no-op。
fn migrate_legacy_secrets(config: &mut AppConfig) -> usize {
    migrate_legacy_secrets_with(config, &|key, value| set_secret(key, value))
}

/// 注入式核心：`write` 可被单测替换为 fake（`keyring` 无法直接 mock），
/// 以断言「写成功 → 清除；写失败 → 保留」。
fn migrate_legacy_secrets_with(
    config: &mut AppConfig,
    write: &dyn Fn(&str, &str) -> Result<()>,
) -> usize {
    let mut migrated = 0usize;
    for c in config.connections.iter_mut() {
        if let Some(ssh) = c.ssh.as_mut() {
            let mut cleared = false;
            if let Some(pw) = ssh.password.as_deref() {
                match write(&secret_key(&c.id, SecretKind::SshPassword), pw) {
                    Ok(()) => {
                        ssh.password = None;
                        cleared = true;
                    }
                    Err(e) => log::warn!("迁移 SSH 密码失败（保留明文）: {e}"),
                }
            }
            if let Some(key_data) = ssh.private_key.as_deref() {
                match write(&secret_key(&c.id, SecretKind::SshPrivateKey), key_data) {
                    Ok(()) => {
                        ssh.private_key = None;
                        cleared = true;
                    }
                    Err(e) => log::warn!("迁移 SSH 私钥失败（保留明文）: {e}"),
                }
            }
            if cleared {
                migrated += 1;
            }
        }
    }
    migrated
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use dby_core::config::{AppConfig, ConnectionConfig};
    use dby_core::driver::SshOptions;
    use dby_core::error::DbError;

    use super::migrate_legacy_secrets_with;

    fn ssh_config(id: &str, password: Option<&str>, private_key: Option<&str>) -> ConnectionConfig {
        ConnectionConfig {
            id: id.to_string(),
            project_id: "p1".to_string(),
            name: "demo".to_string(),
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            database: None,
            ssl: None,
            ssh: Some(SshOptions {
                enabled: true,
                host: "ssh.example.com".to_string(),
                user: "ubuntu".to_string(),
                password: password.map(str::to_string),
                private_key: private_key.map(str::to_string),
                ..Default::default()
            }),
            color: None,
            params: std::collections::HashMap::new(),
        }
    }

    /// 写 keyring 成功 → 明文清除且写入正确键；二次运行 no-op（幂等）（#22）。
    #[test]
    fn migrate_clears_legacy_ssh_plaintext() {
        let mut config = AppConfig {
            connections: vec![ssh_config("c1", Some("ssh-pw"), Some("ssh-key"))],
            ..Default::default()
        };
        let written: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let write = |key: &str, _value: &str| {
            written.borrow_mut().push(key.to_string());
            Ok(())
        };

        let migrated = migrate_legacy_secrets_with(&mut config, &write);

        assert_eq!(migrated, 1);
        let ssh = config.connections[0].ssh.as_ref().unwrap();
        assert!(ssh.password.is_none(), "写成功后应清除明文密码");
        assert!(ssh.private_key.is_none(), "写成功后应清除明文私钥");
        assert_eq!(
            *written.borrow(),
            vec!["c1:ssh".to_string(), "c1:ssh_key".to_string()]
        );

        // 幂等：二次运行不再写、不再计数
        let migrated_again = migrate_legacy_secrets_with(&mut config, &write);
        assert_eq!(migrated_again, 0);
        assert_eq!(written.borrow().len(), 2);
    }

    /// 写 keyring 失败 → 保留明文不丢、不计数，跳过该连接（#22）。
    #[test]
    fn migrate_keeps_plaintext_on_keyring_failure() {
        let mut config = AppConfig {
            connections: vec![ssh_config("c1", Some("ssh-pw"), Some("ssh-key"))],
            ..Default::default()
        };
        let write = |_key: &str, _value: &str| Err(DbError::Other("keyring down".to_string()));

        let migrated = migrate_legacy_secrets_with(&mut config, &write);

        assert_eq!(migrated, 0);
        let ssh = config.connections[0].ssh.as_ref().unwrap();
        assert_eq!(ssh.password.as_deref(), Some("ssh-pw"), "写失败应保留明文");
        assert_eq!(
            ssh.private_key.as_deref(),
            Some("ssh-key"),
            "写失败应保留明文"
        );
    }

    /// 单条失败不影响另一条：密码写失败保留、私钥写成功清除（#22）。
    #[test]
    fn migrate_partial_failure_clears_only_successful_field() {
        let mut config = AppConfig {
            connections: vec![ssh_config("c1", Some("ssh-pw"), Some("ssh-key"))],
            ..Default::default()
        };
        let write = |key: &str, _value: &str| {
            if key.ends_with(":ssh") {
                Err(DbError::Other("keyring down".to_string()))
            } else {
                Ok(())
            }
        };

        let migrated = migrate_legacy_secrets_with(&mut config, &write);

        assert_eq!(migrated, 1);
        let ssh = config.connections[0].ssh.as_ref().unwrap();
        assert_eq!(ssh.password.as_deref(), Some("ssh-pw"), "密码写失败应保留");
        assert!(ssh.private_key.is_none(), "私钥写成功应清除");
    }
}
