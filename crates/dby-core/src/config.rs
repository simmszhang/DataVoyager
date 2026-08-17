//! 应用配置（项目 / 连接 / 设置）的 JSON 持久化。
//!
//! 密码/私钥不落此文件，存 OS 钥匙串（由壳层实现）；这里只存引用与配置元数据。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::driver::{SshOptions, SslOptions};
use crate::error::Result;
use crate::project::Project;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub driver: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<SslOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl ConnectionConfig {
    pub fn new(project_id: impl Into<String>, name: impl Into<String>, driver: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            name: name.into(),
            driver: driver.into(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: String::new(),
            database: None,
            ssl: None,
            ssh: None,
            color: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub default_query_timeout_ms: u64,
    pub history_retention_days: u32,
    pub capture_history: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_query_timeout_ms: 30_000,
            history_retention_days: 90,
            capture_history: true,
            theme: "dark".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub projects: Vec<Project>,
    pub connections: Vec<ConnectionConfig>,
    pub settings: Settings,
}

impl AppConfig {
    /// 读取配置；不存在时返回带"默认项目"的新配置。
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::with_default_project());
        }
        let data = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&data)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn with_default_project() -> Self {
        Self {
            projects: vec![Project::new("默认项目")],
            connections: vec![],
            settings: Settings::default(),
        }
    }

    pub fn find_project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn find_connection(&self, id: &str) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// 某项目下的连接。
    pub fn connections_for_project(&self, project_id: &str) -> Vec<&ConnectionConfig> {
        self.connections
            .iter()
            .filter(|c| c.project_id == project_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_one_project() {
        let cfg = AppConfig::with_default_project();
        assert_eq!(cfg.projects.len(), 1);
        assert!(cfg.connections.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("dby-core-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.json");
        let cfg = AppConfig::with_default_project();
        cfg.save(&path).unwrap();
        let loaded = AppConfig::load(&path).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "默认项目");
        std::fs::remove_dir_all(&dir).ok();
    }
}
