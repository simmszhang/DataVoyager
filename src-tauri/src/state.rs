use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dby_core::config::AppConfig;
use dby_core::driver::{ConnectParams, Connection, DriverRegistry};
use dby_core::history::HistoryStore;
use dby_core::query::CancellationToken;
use tokio::sync::Mutex;

/// 一条活跃连接，归属某项目（S1，design §4.1）。
pub struct ActiveConnection {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    /// 连接参数（secrets 仅存内存）：毒化后自动重连所需。
    #[allow(dead_code)] // Task 5 `ensure_connected` 才读取；当前仅写入
    pub params: ConnectParams,
    /// 秒断（取消关 socket）后置 true，下次使用前由 `ensure_connected` 重连。
    #[allow(dead_code)] // Task 5 `ensure_connected` 才读取；当前仅写入
    pub needs_reconnect: bool,
    pub conn: Box<dyn Connection + Send>,
}

/// 全局应用状态，通过 `Arc` 共享给各命令（S1，design §4.1）。
pub struct AppState {
    pub registry: DriverRegistry,
    /// 外层注册表：`std::sync::Mutex` 只做同步 get/clone/insert/remove，绝不跨 await；
    /// 每条连接一把 `futures::lock::Mutex`（guard Send），可跨 await 持有。
    pub connections: std::sync::Mutex<HashMap<u64, Arc<futures::lock::Mutex<ActiveConnection>>>>,
    /// 查询实例 token 注册表：`"{conn_id}:{query_id}" -> Arc<CancellationToken>`；
    /// Arc 包裹以便 RAII guard 克隆持有（Task 4 填充，当前为空）。
    pub query_tokens: Arc<std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>>,
    pub next_id: AtomicU64,
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    pub history: HistoryStore,
}

impl AppState {
    pub fn new(config: AppConfig, history: HistoryStore, config_path: PathBuf) -> Self {
        let mut registry = DriverRegistry::new();
        registry.register(Arc::new(dby_driver_mysql::MysqlDriver));
        Self {
            registry,
            connections: std::sync::Mutex::new(HashMap::new()),
            query_tokens: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
