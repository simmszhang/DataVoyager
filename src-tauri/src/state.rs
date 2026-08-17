use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use dby_core::config::AppConfig;
use dby_core::driver::{Connection, DriverRegistry};
use dby_core::history::HistoryStore;
use dby_core::query::CancellationToken;
use tokio::sync::Mutex;

/// 一条活跃连接，归属某项目，带独立取消令牌。
pub struct ActiveConnection {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    pub cancel: CancellationToken,
    pub conn: Box<dyn Connection + Send>,
}

/// 全局应用状态，通过 `Arc` 共享给各命令。
pub struct AppState {
    pub registry: DriverRegistry,
    pub connections: Mutex<HashMap<u64, ActiveConnection>>,
    pub next_id: AtomicU64,
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    pub history: HistoryStore,
}

impl AppState {
    pub fn new(config: AppConfig, history: HistoryStore, config_path: PathBuf) -> Self {
        let mut registry = DriverRegistry::new();
        registry.register(std::sync::Arc::new(dby_driver_mysql::MysqlDriver));
        Self {
            registry,
            connections: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            config: Mutex::new(config),
            config_path,
            history,
        }
    }

    pub fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// 解析项目 id：显式指定优先，否则用第一个项目。
    pub async fn resolve_project_id(&self, requested: Option<String>) -> String {
        if let Some(id) = requested.filter(|s| !s.trim().is_empty()) {
            return id;
        }
        self.config
            .lock()
            .await
            .projects
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_default()
    }
}
