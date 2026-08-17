//! 驱动抽象：`Driver` / `Connection` trait、能力矩阵、注册表。

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::dialect::Dialect;
use crate::error::{DbError, Result};
use crate::metadata::{
    ColumnInfo, ForeignKeyInfo, IndexInfo, ProcedureInfo, TableInfo, TriggerInfo,
};
use crate::query::{ExecOpts, QueryOutput};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConnectParams {
    #[serde(default)]
    pub driver: String,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub ssl: Option<SslOptions>,
    #[serde(default)]
    pub ssh: Option<SshOptions>,
    /// 驱动特定参数
    #[serde(default)]
    pub params: HashMap<String, String>,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    3306
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SslOptions {
    pub enabled: bool,
    #[serde(default)]
    pub verify_cert: bool,
    #[serde(default)]
    pub ca_path: Option<String>,
    #[serde(default)]
    pub client_cert: Option<String>,
    #[serde(default)]
    pub client_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshOptions {
    pub enabled: bool,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub private_key: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// 能力矩阵：声明驱动/连接能做什么，前端据此自适应 UI。
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub supports_sql: bool,
    pub supports_transactions: bool,
    pub supports_catalogs: bool,
    pub supports_schemas: bool,
    pub supports_procedures: bool,
    pub supports_cancel: bool,
    pub supports_data_edit: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            supports_sql: true,
            supports_transactions: false,
            supports_catalogs: false,
            supports_schemas: false,
            supports_procedures: false,
            supports_cancel: false,
            supports_data_edit: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DriverInfo {
    pub id: String,
    pub display_name: String,
    pub capabilities: Capabilities,
}

/// 一条活跃连接。
pub trait Connection: Send {
    fn ping(&mut self) -> Result<()>;
    fn server_version(&self) -> String;

    fn catalogs(&mut self) -> Result<Vec<String>>;
    fn schemas(&mut self, catalog: Option<&str>) -> Result<Vec<String>>;
    fn tables(&mut self, schema: &str) -> Result<Vec<TableInfo>>;
    fn columns(&mut self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>>;
    fn indexes(&mut self, schema: &str, table: &str) -> Result<Vec<IndexInfo>>;
    fn foreign_keys(&mut self, schema: &str, table: &str) -> Result<Vec<ForeignKeyInfo>>;
    fn triggers(&mut self, schema: &str, table: &str) -> Result<Vec<TriggerInfo>>;
    fn procedures(&mut self, schema: &str) -> Result<Vec<ProcedureInfo>>;
    fn table_ddl(&mut self, schema: &str, table: &str) -> Result<String>;

    fn execute(&mut self, schema: Option<&str>, sql: &str, opts: &ExecOpts) -> Result<QueryOutput>;

    fn begin(&mut self) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
    fn cancel(&self) -> Result<()>;
}

/// 可打开某类数据库连接的驱动。
pub trait Driver: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> &dyn Dialect;
    fn connect(&self, params: &ConnectParams) -> Result<Box<dyn Connection + Send>>;
}

/// 驱动注册表，按 id 索引。新增数据库 = 实现 `Driver` + 在这里登记。
#[derive(Default)]
pub struct DriverRegistry {
    drivers: HashMap<&'static str, Arc<dyn Driver>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, driver: Arc<dyn Driver>) {
        self.drivers.insert(driver.id(), driver);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Driver>> {
        self.drivers.get(id).cloned()
    }

    pub fn list(&self) -> Vec<DriverInfo> {
        let mut v: Vec<DriverInfo> = self
            .drivers
            .values()
            .map(|d| DriverInfo {
                id: d.id().to_string(),
                display_name: d.display_name().to_string(),
                capabilities: d.capabilities(),
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn default_id(&self) -> Option<String> {
        self.list().into_iter().map(|d| d.id).next()
    }

    pub fn resolve(&self, requested: &str) -> Result<Arc<dyn Driver>> {
        if !requested.trim().is_empty() {
            return self
                .get(requested.trim())
                .ok_or_else(|| DbError::DriverNotFound(requested.to_string()));
        }
        match self.default_id() {
            Some(id) => self.get(&id).ok_or(DbError::DriverNotFound(id)),
            None => Err(DbError::DriverNotFound("<none>".to_string())),
        }
    }
}
