use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use dby_core::config::ConnectionConfig;
use dby_core::danger::DangerLevel;
use dby_core::ddl::ColumnDef;
use dby_core::driver::{execute_buffered, ConnectParams, Driver, DriverInfo};
use dby_core::error::{DbError, Result};
use dby_core::history::{ExecutionRecord, HistoryFilter, StatementHit};
use dby_core::metadata::{ColumnInfo, ColumnType, TableInfo, ViewInfo, TriggerInfo, ProcedureInfo};
use dby_core::project::Project;
use dby_core::query::{
    CancellationToken, ExecOpts, QueryOutput, ResultSink, SqlOrigin, StreamEvent,
};
use tauri::ipc::Channel;

use crate::secrets::{delete_secret, get_secret, secret_key, set_secret, SecretKind};
use crate::state::{ActiveConnection, AppState};

#[derive(Serialize)]
pub struct ConnectResponse {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    /// 关联的保存配置 ID（R11）：save=true 时返回，用于后续重连。
    pub config_id: Option<String>,
}

#[derive(Serialize)]
pub struct ConnectionSummary {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    /// 关联的保存配置 ID（R11）：用于断开后重连。
    pub config_id: Option<String>,
}

/// 已保存连接的脱敏视图：仅暴露非敏感字段，绝不携带密码/私钥（#22）。
#[derive(Serialize)]
pub struct SavedConnectionView {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub driver: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: Option<String>,
    pub has_ssh: bool,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_user: Option<String>,
    pub color: Option<String>,
}

fn snapshot(a: &ActiveConnection) -> ConnectionSummary {
    ConnectionSummary {
        id: a.id,
        name: a.name.clone(),
        driver_id: a.driver_id.clone(),
        project_id: a.project_id.clone(),
        database: a.database.clone(),
        server_version: a.server_version.clone(),
        config_id: a.config_id.clone(), // R11
    }
}

/// 毒化自动重连（#5，design §4.7）：秒断（取消关 socket）后 `needs_reconnect = true`，
/// 下一次使用该连接前用保存的 `params` 重连，刷新 server_version 并清除毒化标记。
/// 各使用连接的命令在 `entry.lock().await` 之后先调用本函数。
async fn ensure_connected(state: &Arc<AppState>, active: &mut ActiveConnection) -> Result<()> {
    if active.needs_reconnect {
        let driver = state.registry.resolve(&active.driver_id)?;
        let conn = driver.connect(&active.params).await?;
        active.conn = conn;
        active.server_version = active.conn.server_version();
        active.needs_reconnect = false;
    }
    Ok(())
}

#[tauri::command]
pub fn list_drivers(state: State<'_, Arc<AppState>>) -> Vec<DriverInfo> {
    state.registry.list()
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, Arc<AppState>>,
    params: ConnectParams,
) -> Result<String> {
    // SSH 隧道开启但尚无 TOFU 指纹：要求前端先走探针确认（probe_host_key）
    if let Some(ssh) = &params.ssh {
        if ssh.enabled && ssh.host_key_fingerprint.is_none() {
            return Err(DbError::Config("需先确认 SSH 主机指纹".to_string()));
        }
    }
    let driver = state.registry.resolve(&params.driver)?;
    let conn = driver.connect(&params).await?;
    Ok(conn.server_version())
}

/// 只读探针：连 SSH 完成 kex 取主机公钥指纹即断开（不认证、不转发），
/// 供前端首次连接展示 `SHA256:…` 并确认（TOFU）。
#[tauri::command]
pub async fn probe_host_key(params: ConnectParams) -> Result<String> {
    let ssh = params
        .ssh
        .as_ref()
        .ok_or_else(|| DbError::Config("未配置 SSH".to_string()))?;
    dby_driver_mysql::probe_host_key(ssh).await
}

#[tauri::command]
pub async fn connect(
    state: State<'_, Arc<AppState>>,
    params: ConnectParams,
    project_id: Option<String>,
    save: bool,
    remember_password: bool,
) -> Result<ConnectResponse> {
    let project_id = state.resolve_project_id(project_id).await;
    let name = auto_name(&params);
    let resp = open_session(state.inner(), &params, project_id.clone(), name.clone()).await?;

    // 先建会话，再按 save/remember_password 决定是否持久化（#6/#43）。
    let config = ConnectionConfig {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.clone(),
        name: name.clone(),
        driver: params.driver.clone(),
        host: params.host.clone(),
        port: params.port,
        user: params.user.clone(),
        database: params.database.clone(),
        ssl: params.ssl.clone(),
        ssh: params.ssh.clone(),
        color: None,
        params: params.params.clone(),
    };
    let config_id = persist_connection(
        state.inner(),
        resp.id,
        config,
        &params,
        save,
        remember_password,
        SecretsIo {
            store: store_secrets,
            delete: delete_secrets,
        },
    )
    .await?;
    
    // R11: 更新 ActiveConnection 的 config_id
    if let Some(ref cid) = config_id {
        let conn_lock = {
            let guard = state.connections.lock().unwrap();
            guard.get(&resp.id).cloned()
        };
        if let Some(conn_lock) = conn_lock {
            let mut conn = conn_lock.lock().await;
            conn.config_id = Some(cid.clone());
        }
    }
    
    Ok(ConnectResponse {
        id: resp.id,
        name,
        driver_id: resp.driver_id,
        project_id,
        database: resp.database,
        server_version: resp.server_version,
        config_id,
    })
}

/// 钥匙串读写单元：`store_secrets` / `delete_secrets` 的注入点。
/// `keyring::Entry` 无法直接 mock，单测传入 fake 以断言「save=false 不写」。
struct SecretsIo {
    store: fn(&str, &ConnectParams) -> Result<()>,
    delete: fn(&str),
}

/// 连接成功后的持久化控制流（#6/#43）：
///
/// - `save=false`：不写 config、不写 keyring（no-op），返回 `None`。
/// - `save=true`：secrets 先存（`remember_password` 时）、config 后存；任一步失败都回滚已写内容并断开会话，不留半失败态。返回 `Some(config_id)`。
///
/// `secrets` 是钥匙串读写单元，真实实现为 `store_secrets` / `delete_secrets`。
async fn persist_connection(
    state: &Arc<AppState>,
    resp_id: u64,
    config: ConnectionConfig,
    params: &ConnectParams,
    save: bool,
    remember_password: bool,
    secrets: SecretsIo,
) -> Result<Option<String>> {
    if !save {
        return Ok(None);
    }
    let config_id = config.id.clone();
    // secrets 先存、config 后存：避免「config 已存但 secret 缺失」半失败态
    if remember_password {
        if let Err(e) = (secrets.store)(&config_id, params) {
            state.connections.lock().unwrap().remove(&resp_id); // 断开不留孤儿；未存 config，可安全失败
            return Err(e);
        }
    }
    if let Err(e) = persist_config(state, config).await {
        // config 保存失败：回滚已存 secrets + 断开（不留半失败态）
        if remember_password {
            (secrets.delete)(&config_id);
        }
        state.connections.lock().unwrap().remove(&resp_id);
        return Err(e);
    }
    Ok(Some(config_id))
}

/// 把连接配置追加到内存并落盘；save 失败回滚内存 push（不留幽灵配置），
/// 并映射为 `DbError::Storage`（非静默，#39/#43）。
async fn persist_config(state: &Arc<AppState>, config: ConnectionConfig) -> Result<()> {
    let mut cfg = state.config.lock().await;
    let id = config.id.clone();
    cfg.connections.push(config);
    cfg.save(&state.config_path).map_err(|e| {
        cfg.connections.retain(|c| c.id != id);
        DbError::Storage(format!("保存连接配置失败: {e}"))
    })
}

/// 把连接参数里的敏感值写入钥匙串三键：
/// `{config_id}`（MySQL 密码）、`{config_id}:ssh`（SSH 密码）、`{config_id}:ssh_key`（SSH 私钥）。
/// None/空串跳过不写；写失败 `log::warn!` + 返回 `DbError::Storage`（非静默，#39）。
fn store_secrets(config_id: &str, params: &ConnectParams) -> Result<()> {
    if let Some(pw) = params.password.as_deref() {
        if !pw.is_empty() {
            let key = secret_key(config_id, SecretKind::MysqlPassword);
            if let Err(e) = set_secret(&key, pw) {
                log::warn!("写入钥匙串条目 {key} 失败: {e}");
                return Err(DbError::Storage(format!("保存 MySQL 密码失败: {e}")));
            }
        }
    }
    if let Some(ssh) = &params.ssh {
        if let Some(pw) = ssh.password.as_deref() {
            if !pw.is_empty() {
                let key = secret_key(config_id, SecretKind::SshPassword);
                if let Err(e) = set_secret(&key, pw) {
                    log::warn!("写入钥匙串条目 {key} 失败: {e}");
                    return Err(DbError::Storage(format!("保存 SSH 密码失败: {e}")));
                }
            }
        }
        if let Some(key_data) = ssh.private_key.as_deref() {
            if !key_data.is_empty() {
                let key = secret_key(config_id, SecretKind::SshPrivateKey);
                if let Err(e) = set_secret(&key, key_data) {
                    log::warn!("写入钥匙串条目 {key} 失败: {e}");
                    return Err(DbError::Storage(format!("保存 SSH 私钥失败: {e}")));
                }
            }
        }
    }
    Ok(())
}

/// 删除三键（回滚/级联清理用）；best-effort，单键失败仅 `log::warn!`。
fn delete_secrets(config_id: &str) {
    for (kind, label) in [
        (SecretKind::MysqlPassword, "MySQL 密码"),
        (SecretKind::SshPassword, "SSH 密码"),
        (SecretKind::SshPrivateKey, "SSH 私钥"),
    ] {
        let key = secret_key(config_id, kind);
        if let Err(e) = delete_secret(&key) {
            log::warn!("删除钥匙串条目 {key}（{label}）失败: {e}");
        }
    }
}

/// 打开会话（连接 + 建 ActiveConnection），返回响应。
async fn open_session(
    state: &Arc<AppState>,
    params: &ConnectParams,
    project_id: String,
    name: String,
) -> Result<ConnectResponse> {
    let driver = state.registry.resolve(&params.driver)?;
    let conn = driver.connect(params).await?;
    let server_version = conn.server_version();
    let database = params.database.clone().unwrap_or_default();
    let id = state.alloc_id();
    let active = ActiveConnection {
        id,
        name: name.clone(),
        driver_id: driver.id().to_string(),
        project_id: project_id.clone(),
        database: database.clone(),
        server_version: server_version.clone(),
        config_id: None, // R11: 初始为 None，connect 命令会在保存后更新
        params: params.clone(),
        needs_reconnect: false,
        conn,
    };
    state
        .connections
        .lock()
        .unwrap()
        .insert(id, Arc::new(futures::lock::Mutex::new(active)));
    Ok(ConnectResponse {
        id,
        name,
        driver_id: driver.id().to_string(),
        project_id,
        database,
        server_version,
        config_id: None, // R11: open_session 不涉及持久化，由 connect 更新
    })
}

fn auto_name(params: &ConnectParams) -> String {
    let database = params.database.clone().unwrap_or_default();
    format!(
        "{}@{}:{}{}",
        params.user,
        params.host,
        params.port,
        if database.is_empty() {
            String::new()
        } else {
            format!("/{database}")
        }
    )
}

#[tauri::command]
pub async fn list_saved_connections(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
) -> Result<Vec<SavedConnectionView>> {
    let cfg = state.config.lock().await;
    Ok(cfg
        .connections
        .iter()
        .filter(|c| {
            project_id
                .as_deref()
                .map(|p| c.project_id == p)
                .unwrap_or(true)
        })
        .map(|c| SavedConnectionView {
            id: c.id.clone(),
            project_id: c.project_id.clone(),
            name: c.name.clone(),
            driver: c.driver.clone(),
            host: c.host.clone(),
            port: c.port,
            user: c.user.clone(),
            database: c.database.clone(),
            has_ssh: c.ssh.as_ref().map(|s| s.enabled).unwrap_or(false),
            ssh_host: c.ssh.as_ref().map(|s| s.host.clone()),
            ssh_port: c.ssh.as_ref().map(|s| s.port),
            ssh_user: c.ssh.as_ref().map(|s| s.user.clone()),
            color: c.color.clone(),
        })
        .collect())
}

/// 从钥匙串读出的凭据集合（MySQL 密码 + SSH 密码 + SSH 私钥）。
/// 与 keyring 解耦：`build_params_from_config` 只消费它，便于无钥匙串单测（#22）。
#[derive(Debug, Clone, Default)]
struct SshSecrets {
    password: Option<String>,
    ssh_password: Option<String>,
    ssh_private_key: Option<String>,
}

/// 纯映射：把已存连接配置 + 钥匙串凭据拼成 `ConnectParams`（#22/#63）。
/// - `password` 来自 secrets；ssh.password/private_key 覆盖 config 中残留值；
/// - `params` 从 config 回填（驱动特定参数）。
fn build_params_from_config(config: &ConnectionConfig, secrets: &SshSecrets) -> ConnectParams {
    ConnectParams {
        driver: config.driver.clone(),
        host: config.host.clone(),
        port: config.port,
        user: config.user.clone(),
        password: secrets.password.clone(),
        database: config.database.clone(),
        ssl: config.ssl.clone(),
        ssh: config.ssh.clone().map(|mut s| {
            s.password = secrets.ssh_password.clone();
            s.private_key = secrets.ssh_private_key.clone();
            s
        }),
        params: config.params.clone(),
    }
}

#[tauri::command]
pub async fn reconnect(
    state: State<'_, Arc<AppState>>,
    config_id: String,
) -> Result<ConnectResponse> {
    let config = {
        let cfg = state.config.lock().await;
        cfg.connections.iter().find(|c| c.id == config_id).cloned()
    }
    .ok_or_else(|| DbError::Config("连接配置不存在".to_string()))?;

    // Retrieve secrets from keyring, logging errors for diagnostics (#72)
    let password = match get_secret(&secret_key(&config_id, SecretKind::MysqlPassword)) {
        Ok(pw) => Some(pw),
        Err(e) => {
            log::warn!("Failed to read MySQL password from keyring for {}: {}", config_id, e);
            None
        }
    };
    
    let ssh_password = match get_secret(&secret_key(&config_id, SecretKind::SshPassword)) {
        Ok(pw) => Some(pw),
        Err(e) => {
            log::warn!("Failed to read SSH password from keyring for {}: {}", config_id, e);
            None
        }
    };
    
    let ssh_private_key = match get_secret(&secret_key(&config_id, SecretKind::SshPrivateKey)) {
        Ok(key) => Some(key),
        Err(e) => {
            log::warn!("Failed to read SSH private key from keyring for {}: {}", config_id, e);
            None
        }
    };

    let secrets = SshSecrets {
        password,
        ssh_password,
        ssh_private_key,
    };

    // 缺 secret 判定：SSH 启用但 keyring 无任何 SSH 凭据 → 报错，引导前端走 connect 补录。
    // MySQL 密码缺失不强制（可能无密码库），由驱动认证失败自然报错（§4.5/§7）。
    if config.ssh.as_ref().map(|s| s.enabled).unwrap_or(false)
        && secrets.ssh_password.is_none()
        && secrets.ssh_private_key.is_none()
    {
        return Err(DbError::Config(
            "该连接未保存 SSH 凭据，请重新连接并输入".to_string(),
        ));
    }

    let params = build_params_from_config(&config, &secrets);
    let mut resp = open_session(
        state.inner(),
        &params,
        config.project_id.clone(),
        config.name.clone(),
    )
    .await?;
    
    // R11: 更新 ActiveConnection 的 config_id（重连时复用已有配置）
    let conn_lock = {
        let guard = state.connections.lock().unwrap();
        guard.get(&resp.id).cloned()
    };
    if let Some(conn_lock) = conn_lock {
        let mut conn = conn_lock.lock().await;
        conn.config_id = Some(config_id.clone());
    }
    resp.config_id = Some(config_id);
    
    Ok(resp)
}

#[tauri::command]
pub async fn delete_saved_connection(
    state: State<'_, Arc<AppState>>,
    config_id: String,
) -> Result<()> {
    {
        let mut cfg = state.config.lock().await;
        cfg.connections.retain(|c| c.id != config_id);
        cfg.save(&state.config_path)?;
    }
    delete_secrets(&config_id);
    Ok(())
}

/// 已保存连接的更新载荷（design §4.7，#63）：仅非敏感字段——
/// 敏感凭据（MySQL 密码 / SSH 密码 / SSH 私钥）走 `store_secrets` 单独更新，不进此结构。
#[derive(Deserialize)]
pub struct UpdateSavedConnection {
    pub name: Option<String>,
    pub color: Option<String>,
    pub ssh: Option<dby_core::driver::SshOptions>,
}

/// 纯映射：把 `UpdateSavedConnection` 的 Some 字段覆盖到 config；
/// None 字段保持原值；`id` 等标识字段绝不被改写（#63）。
fn apply_update(config: &mut ConnectionConfig, update: &UpdateSavedConnection) {
    if let Some(name) = &update.name {
        config.name = name.clone();
    }
    if let Some(color) = &update.color {
        config.color = Some(color.clone());
    }
    if let Some(ssh) = &update.ssh {
        config.ssh = Some(ssh.clone());
    }
}

#[tauri::command]
pub async fn update_saved_connection(
    state: State<'_, Arc<AppState>>,
    config_id: String,
    update: UpdateSavedConnection,
) -> Result<()> {
    let mut cfg = state.config.lock().await;
    let config = cfg
        .connections
        .iter_mut()
        .find(|c| c.id == config_id)
        .ok_or_else(|| DbError::Config("连接配置不存在".to_string()))?;
    apply_update(config, &update);
    cfg.save(&state.config_path)?;
    Ok(())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    state.connections.lock().unwrap().remove(&id);
    Ok(())
}

#[tauri::command]
pub async fn list_connections(state: State<'_, Arc<AppState>>) -> Result<Vec<ConnectionSummary>> {
    // 外层同步锁只借 Arc（快照），立即释放；逐连接加 per-connection 锁再快照（S1，design §4.2）。
    let entries: Vec<Arc<futures::lock::Mutex<ActiveConnection>>> = {
        let guard = state.connections.lock().unwrap();
        guard.values().cloned().collect()
    };
    let mut v: Vec<ConnectionSummary> = Vec::with_capacity(entries.len());
    for entry in entries {
        let active = entry.lock().await;
        v.push(snapshot(&active));
    }
    v.sort_by_key(|c| c.id);
    Ok(v)
}

#[tauri::command]
pub async fn list_databases(state: State<'_, Arc<AppState>>, id: u64) -> Result<Vec<String>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.schemas(None).await
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<TableInfo>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.tables(&database).await
}

#[tauri::command]
pub async fn list_columns(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<ColumnInfo>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.columns(&database, &table).await
}

#[tauri::command]
pub async fn list_views(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<ViewInfo>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.views(&database).await
}

#[tauri::command]
pub async fn list_functions(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<ProcedureInfo>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    let all = active.conn.procedures(&database).await?;
    Ok(all.into_iter().filter(|p| p.kind == "FUNCTION").collect())
}

#[tauri::command]
pub async fn list_procedures(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<ProcedureInfo>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    let all = active.conn.procedures(&database).await?;
    Ok(all.into_iter().filter(|p| p.kind == "PROCEDURE").collect())
}

#[tauri::command]
pub async fn list_triggers(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<TriggerInfo>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.triggers(&database, None).await
}

/// 表浏览 SQL 生成（#4）：SQL 由 dby-core 的 `Dialect` 生成，前端只传结构化参数。
/// 遵循 S1 持锁范式：外层同步锁只快照 Arc，per-connection 锁只读 `driver_id`，不跨其它 await。
#[tauri::command]
pub async fn build_table_select(
    state: State<'_, Arc<AppState>>,
    id: u64,
    table: String,
) -> Result<String> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let active = entry.lock().await; // 只读 driver_id
    let driver = state.registry.resolve(&active.driver_id)?;
    Ok(dby_core::query::build_table_select(
        driver.dialect(),
        &table,
        Some(100),
    ))
}

#[tauri::command]
pub async fn execute_query(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: Option<String>,
    sql: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    guard_dangerous(&sql, confirmed)?;
    let started = Utc::now();
    // 查询实例 token：注册到全局注册表（Arc 共享内部状态），RAII 守卫保证任何退出路径（含 `?` 早退）自动注销（#23）。
    let query_id = uuid::Uuid::new_v4().to_string();
    let key = format!("{id}:{query_id}");
    let token = CancellationToken::new();
    state
        .query_tokens
        .lock()
        .unwrap()
        .insert(key.clone(), Arc::new(token.clone()));
    let _guard = QueryTokenGuard {
        map: state.query_tokens.clone(),
        key,
    };
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    let result = execute_buffered(
        active.conn.as_mut(),
        database.as_deref(),
        &sql,
        &ExecOpts {
            cancel: Some(token),
            ..Default::default()
        },
    )
    .await;

    let duration_ms = (Utc::now() - started).num_milliseconds().max(0) as u64;
    let mut rec = ExecutionRecord::new(project_id, sql.clone(), SqlOrigin::ManualEditor);
    rec.connection_id = Some(id.to_string());
    rec.connection_name = Some(conn_name);
    rec.database = database.clone();
    rec.duration_ms = duration_ms;

    match result {
        Ok(output) => {
            rec.status = "ok".to_string();
            rec.rows_affected = output.affected_rows;
            rec.row_count = output.first_result_set().map(|rs| rs.rows.len() as u64);
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Ok(output)
        }
        Err(e) => {
            // 秒断：取消即关 socket，连接毒化 → 标记重连；历史 status=cancelled（#5）。
            if matches!(e, DbError::Cancelled) {
                active.needs_reconnect = true;
                rec.status = "cancelled".to_string();
            } else {
                rec.status = e.to_string();
            }
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Err(e)
        }
    }
}

// ---------- 项目 ----------

#[tauri::command]
pub async fn list_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<Project>> {
    Ok(state.config.lock().await.projects.clone())
}

#[tauri::command]
pub async fn create_project(state: State<'_, Arc<AppState>>, name: String) -> Result<Project> {
    let mut cfg = state.config.lock().await;
    let project = Project::new(name);
    cfg.projects.push(project.clone());
    cfg.save(&state.config_path)?;
    Ok(project)
}

#[tauri::command]
pub async fn rename_project(
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
) -> Result<Project> {
    let mut cfg = state.config.lock().await;
    let project = cfg
        .projects
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| DbError::Config("project not found".to_string()))?;
    project.name = name;
    project.touch();
    let result = project.clone();
    cfg.save(&state.config_path)?;
    Ok(result)
}

#[tauri::command]
pub async fn delete_project(
    state: State<'_, Arc<AppState>>,
    id: String,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(DbError::Config("危险操作需二次确认".to_string()));
    }
    delete_project_impl(state.inner(), &id).await
}

/// 删除项目（R1，design §4.6）：活跃连接或「已保存连接」（config.connections，非仅活跃）任一存在即
/// 返回 `DbError::Config` 拒绝（不级联删除，避免误删其它项目在用凭据，#41）。
async fn delete_project_impl(state: &Arc<AppState>, id: &str) -> Result<()> {
    // 遍历活跃连接：外层同步锁只快照 Arc 列表，逐连接加 per-connection 锁判 project_id（S1，design §4.2）。
    let entries: Vec<Arc<futures::lock::Mutex<ActiveConnection>>> = {
        let guard = state.connections.lock().unwrap();
        guard.values().cloned().collect()
    };
    for entry in entries {
        let active = entry.lock().await;
        if active.project_id == id {
            return Err(DbError::Config("项目下仍有连接，请先删除连接".to_string()));
        }
    }
    {
        let cfg = state.config.lock().await;
        if cfg.connections.iter().any(|c| c.project_id == id) {
            return Err(DbError::Config(
                "项目下仍有已保存连接，请先删除连接".to_string(),
            ));
        }
    }
    let mut cfg = state.config.lock().await;
    cfg.projects.retain(|p| p.id != id);
    cfg.save(&state.config_path)?;
    state.history.clear(Some(id))?;
    Ok(())
}

// ---------- 历史 ----------

#[tauri::command]
pub async fn search_history(
    state: State<'_, Arc<AppState>>,
    query: String,
    project_id: Option<String>,
) -> Result<Vec<StatementHit>> {
    let mut filter = HistoryFilter::new();
    filter.project_id = project_id;
    state.history.search(&query, &filter)
}

#[tauri::command]
pub async fn list_history(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
) -> Result<Vec<StatementHit>> {
    let mut filter = HistoryFilter::new();
    filter.project_id = project_id;
    state.history.statements(&filter)
}

#[tauri::command]
pub async fn list_executions(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
) -> Result<Vec<ExecutionRecord>> {
    let mut filter = HistoryFilter::new();
    filter.project_id = project_id;
    state.history.executions(&filter)
}

#[tauri::command]
pub async fn pin_statement(
    state: State<'_, Arc<AppState>>,
    hash: String,
    pinned: bool,
) -> Result<()> {
    state.history.pin_statement(&hash, pinned)
}

#[tauri::command]
pub async fn delete_execution(state: State<'_, Arc<AppState>>, id: String) -> Result<()> {
    state.history.delete_execution(&id)
}

// ---------- 流式查询 + 取消 ----------

/// 把流式事件转发到 Tauri Channel，并累计行数（供历史记录）。
struct ChannelSink {
    channel: Channel<StreamEvent>,
    rows: usize,
    /// 查询实例 token（#5/S1）：channel send 失败（前端已关闭）时主动取消后端（#42）。
    cancel: CancellationToken,
}

impl ResultSink for ChannelSink {
    fn on_event(&mut self, ev: StreamEvent) {
        if let StreamEvent::Rows(ref rows) = ev {
            self.rows += rows.len();
        }
        if let Err(e) = self.channel.send(ev) {
            log::warn!("转发流式事件到前端失败: {e}");
            // 前端已关闭 channel（#42）：主动取消查询，避免后端继续执行成无主孤儿。
            self.cancel.cancel();
        }
    }
}

/// 把 `DbError` 映射为 S5 流式终止事件 kind（前端据此区分「取消」与「失败」，#29）。
fn stream_error_kind(e: &DbError) -> &'static str {
    match e {
        DbError::Cancelled => "cancelled",
        _ => "database",
    }
}

#[tauri::command]
pub async fn execute_query_stream(
    state: State<'_, Arc<AppState>>,
    channel: Channel<StreamEvent>,
    id: u64,
    database: Option<String>,
    sql: String,
    confirmed: bool,
) -> Result<()> {
    guard_dangerous(&sql, confirmed)?;
    let started = Utc::now();
    // 查询实例 token：注册到全局注册表（Arc 共享内部状态），RAII 守卫保证任何退出路径（含 `?` 早退）自动注销（#23）。
    let query_id = uuid::Uuid::new_v4().to_string();
    let key = format!("{id}:{query_id}");
    let token = CancellationToken::new();
    state
        .query_tokens
        .lock()
        .unwrap()
        .insert(key.clone(), Arc::new(token.clone()));
    let _guard = QueryTokenGuard {
        map: state.query_tokens.clone(),
        key,
    };
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    let opts = ExecOpts {
        cancel: Some(token.clone()),
        ..Default::default()
    };
    let mut sink = ChannelSink {
        channel,
        rows: 0,
        cancel: token,
    };
    let result = active
        .conn
        .execute_stream(database.as_deref(), &sql, &opts, &mut sink)
        .await;
    let row_count = sink.rows as u64;

    let duration_ms = (Utc::now() - started).num_milliseconds().max(0) as u64;
    let mut rec = ExecutionRecord::new(project_id, sql.clone(), SqlOrigin::ManualEditor);
    rec.connection_id = Some(id.to_string());
    rec.connection_name = Some(conn_name);
    rec.database = database.clone();
    rec.duration_ms = duration_ms;
    rec.row_count = Some(row_count);

    match result {
        Ok(()) => {
            // 成功收尾：经 channel 发 Done（S4），与 invoke 返回值解耦（#28）。
            let _ = sink.channel.send(StreamEvent::Done);
            rec.status = "ok".to_string();
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Ok(())
        }
        Err(e) => {
            // 失败收尾：经 channel 发 Error（携带 S5 kind），invoke 仍返回 Err（#28）。
            let _ = sink.channel.send(StreamEvent::Error {
                kind: stream_error_kind(&e).to_string(),
                message: e.to_string(),
            });
            // 秒断：取消即关 socket，连接毒化 → 标记重连；历史 status=cancelled（#5）。
            if matches!(e, DbError::Cancelled) {
                active.needs_reconnect = true;
                rec.status = "cancelled".to_string();
            } else {
                rec.status = e.to_string();
            }
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Err(e)
        }
    }
}

/// 查询实例 token 的 RAII 注销守卫：离开作用域即从注册表移除，任何退出路径（含 `?` 早退）不泄漏（#23）。
struct QueryTokenGuard {
    map: Arc<std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>>,
    key: String,
}

impl Drop for QueryTokenGuard {
    fn drop(&mut self) {
        self.map.lock().unwrap().remove(&self.key);
    }
}

/// 取消某连接的全部查询实例 token：只读注册表、同步锁不跨 await（#21/#23）。
/// 从 `cancel_query` 抽出以便单测（Tauri `State` 无法在测试中构造）。
fn cancel_queries_for_connection(
    tokens: &std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>,
    id: u64,
) {
    let tokens = tokens.lock().unwrap();
    let prefix = format!("{id}:");
    for (k, t) in tokens.iter() {
        if k.starts_with(&prefix) {
            t.cancel();
        }
    }
}

#[tauri::command]
pub async fn cancel_query(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    // 只读查询实例 token 注册表（不抢连接锁，#21/#23）。
    cancel_queries_for_connection(&state.query_tokens, id);
    Ok(())
}

// ---------- 危险操作分析 ----------

#[tauri::command]
pub fn analyze_danger(sql: String) -> DangerLevel {
    dby_core::danger::analyze_danger(&sql)
}

/// 服务端危险复检（纵深防御）：危险 SQL 且未二次确认 → 拒绝；其余放行。
/// 前端确认弹窗通过后传 `confirmed=true`，绕过前端的调用一律被拦截。
fn guard_dangerous(sql: &str, confirmed: bool) -> Result<()> {
    if dby_core::danger::analyze_danger(sql).is_dangerous() && !confirmed {
        return Err(DbError::Config("危险操作需二次确认".to_string()));
    }
    Ok(())
}

// ---------- 事务 ----------

#[tauri::command]
pub async fn begin(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.begin().await
}

#[tauri::command]
pub async fn commit(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.commit().await
}

#[tauri::command]
pub async fn rollback(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.rollback().await
}

#[tauri::command]
pub async fn set_autocommit(state: State<'_, Arc<AppState>>, id: u64, enabled: bool) -> Result<()> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    active.conn.set_autocommit(enabled).await
}

// ---------- 数据编辑 ----------

/// 把 `(列名, 列类型, 输入串)` 解析为 `(列名, Value)`（design §4.6，#11/#69）。
/// 编辑提交携带「原始输入串 + 列类型」，由 `dby_core::edit::parse_value` 统一按列类型解析，
/// 主键列同样按类型解析（BIGINT/UUID/DATE 主键不再走前端正则启发式）。
fn parse_cells(
    cells: &[(String, ColumnType, String)],
) -> Result<Vec<(String, dby_core::value::Value)>> {
    cells
        .iter()
        .map(|(name, ct, input)| {
            let v = dby_core::edit::parse_value(input, ct)?;
            Ok((name.clone(), v))
        })
        .collect()
}

#[tauri::command]
pub async fn build_edit_sql(
    state: State<'_, Arc<AppState>>,
    id: u64,
    table: String,
    pk: Vec<(String, ColumnType, String)>,
    set: Vec<(String, ColumnType, String)>,
) -> Result<String> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let pk = parse_cells(&pk)?;
    let set = parse_cells(&set)?;
    Ok(dby_core::edit::build_update(
        driver.dialect(),
        &table,
        &pk,
        &set,
    ))
}

#[tauri::command]
pub async fn build_insert_sql(
    state: State<'_, Arc<AppState>>,
    id: u64,
    table: String,
    cells: Vec<(String, ColumnType, String)>,
) -> Result<String> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;

    let parsed = parse_cells(&cells)?;
    let columns: Vec<String> = parsed.iter().map(|(n, _)| n.clone()).collect();
    let values: Vec<dby_core::value::Value> = parsed.into_iter().map(|(_, v)| v).collect();

    Ok(dby_core::edit::build_insert(
        driver.dialect(),
        &table,
        &columns,
        &values,
    ))
}

#[tauri::command]
pub async fn show_create_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<String> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;

    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;

    let driver_id = active.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();
    
    let sql = format!("SHOW CREATE TABLE {}", dialect.quote_identifier(&table));

    let output = execute_buffered(
        active.conn.as_mut(),
        Some(&database),
        &sql,
        &ExecOpts::default(),
    )
    .await?;

    // 提取第一个结果集的第一行第二列（Create Table）
    if let Some(result_set) = output.first_result_set() {
        if let Some(row) = result_set.rows.first() {
            if let Some(cell) = row.get(1) {
                return Ok(cell.to_display_string());
            }
        }
    }

    Err(DbError::Other("No CREATE TABLE result".into()))
}

#[tauri::command]
pub async fn get_primary_key(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<String>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;

    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;

    let driver_id = active.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();

    let sql = format!(
        "SHOW KEYS FROM {} WHERE Key_name = 'PRIMARY'",
        dialect.quote_identifier(&table)
    );

    let output = execute_buffered(
        active.conn.as_mut(),
        Some(&database),
        &sql,
        &ExecOpts::default(),
    )
    .await?;

    // 提取 Column_name（第 5 列，索引 4）
    let mut pk_columns = Vec::new();
    if let Some(result_set) = output.first_result_set() {
        for row in &result_set.rows {
            if let Some(cell) = row.get(4) {
                pk_columns.push(cell.to_display_string());
            }
        }
    }

    Ok(pk_columns)
}

#[tauri::command]
pub async fn batch_delete_rows(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    pk_column: String,
    pk_values: Vec<String>,
    confirmed: bool,
) -> Result<QueryOutput> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;

    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();

    // 生成 DELETE 语句
    let placeholders = pk_values
        .iter()
        .map(|v| dialect.quote_string(v))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "DELETE FROM {} WHERE {} IN ({})",
        dialect.quote_identifier(&table),
        dialect.quote_identifier(&pk_column),
        placeholders
    );

    // 走 guard_dangerous 确认
    guard_dangerous(&sql, confirmed)?;

    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn batch_insert_rows(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    rows: Vec<Vec<(String, ColumnType, String)>>,
) -> Result<QueryOutput> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;

    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();

    if rows.is_empty() {
        return Err(DbError::Other("No rows to insert".into()));
    }

    // 解析所有行并生成多个 INSERT 语句（简化方案：逐行插入）
    let mut sqls = Vec::new();
    for row in &rows {
        let parsed = parse_cells(row)?;
        let columns: Vec<String> = parsed.iter().map(|(n, _)| n.clone()).collect();
        let values: Vec<dby_core::value::Value> = parsed.into_iter().map(|(_, v)| v).collect();
        
        let sql = dby_core::edit::build_insert(dialect, &table, &columns, &values);
        sqls.push(sql);
    }

    // 合并为单个 SQL（使用分号连接）
    let combined_sql = sqls.join(";\n");

    run_ddl(state.inner(), id, Some(database), combined_sql).await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableColumn {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
}

#[tauri::command]
pub async fn get_table_structure(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<TableColumn>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;

    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;

    let driver_id = active.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();

    // SHOW FULL COLUMNS FROM table
    let sql = format!(
        "SHOW FULL COLUMNS FROM {}",
        dialect.quote_identifier(&table)
    );

    let output = execute_buffered(
        active.conn.as_mut(),
        Some(&database),
        &sql,
        &ExecOpts::default(),
    )
    .await?;

    let mut columns = Vec::new();
    if let Some(result_set) = output.first_result_set() {
        for row in &result_set.rows {
            let name = row.first().map(|v| v.to_display_string()).unwrap_or_default();
            let type_name = row.get(1).map(|v| v.to_display_string()).unwrap_or_default();
            let nullable = row
                .get(3)
                .map(|v| v.to_display_string().to_uppercase() == "YES")
                .unwrap_or(true);
            let default_value = row.get(5).and_then(|v| {
                if matches!(v, dby_core::value::Value::Null) {
                    None
                } else {
                    Some(v.to_display_string())
                }
            });
            let comment = row.get(8).and_then(|v| {
                let s = v.to_display_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            });

            columns.push(TableColumn {
                name,
                type_name,
                nullable,
                default_value,
                comment,
            });
        }
    }

    Ok(columns)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum AlterTableOp {
    AddColumn {
        name: String,
        type_name: String,
        nullable: bool,
        default_value: Option<String>,
    },
    DropColumn {
        name: String,
    },
    ModifyColumn {
        name: String,
        type_name: String,
        nullable: bool,
        default_value: Option<String>,
    },
    RenameColumn {
        old_name: String,
        new_name: String,
    },
}

#[tauri::command]
pub async fn alter_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    operations: Vec<AlterTableOp>,
    confirmed: bool,
) -> Result<QueryOutput> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;

    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();

    // 生成 ALTER TABLE 语句
    let mut clauses = Vec::new();
    for op in operations {
        match op {
            AlterTableOp::AddColumn {
                name,
                type_name,
                nullable,
                default_value,
            } => {
                let mut clause = format!(
                    "ADD COLUMN {} {}",
                    dialect.quote_identifier(&name),
                    type_name
                );
                if !nullable {
                    clause.push_str(" NOT NULL");
                }
                if let Some(default) = default_value {
                    clause.push_str(&format!(" DEFAULT {}", dialect.quote_string(&default)));
                }
                clauses.push(clause);
            }
            AlterTableOp::DropColumn { name } => {
                clauses.push(format!("DROP COLUMN {}", dialect.quote_identifier(&name)));
            }
            AlterTableOp::ModifyColumn {
                name,
                type_name,
                nullable,
                default_value,
            } => {
                let mut clause = format!(
                    "MODIFY COLUMN {} {}",
                    dialect.quote_identifier(&name),
                    type_name
                );
                if !nullable {
                    clause.push_str(" NOT NULL");
                }
                if let Some(default) = default_value {
                    clause.push_str(&format!(" DEFAULT {}", dialect.quote_string(&default)));
                }
                clauses.push(clause);
            }
            AlterTableOp::RenameColumn { old_name, new_name } => {
                clauses.push(format!(
                    "RENAME COLUMN {} TO {}",
                    dialect.quote_identifier(&old_name),
                    dialect.quote_identifier(&new_name)
                ));
            }
        }
    }

    if clauses.is_empty() {
        return Err(DbError::Other("No operations specified".into()));
    }

    let sql = format!(
        "ALTER TABLE {} {}",
        dialect.quote_identifier(&table),
        clauses.join(", ")
    );

    // 走 guard_dangerous 确认
    guard_dangerous(&sql, confirmed)?;

    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn execute_edit(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: Option<String>,
    table: String,
    pk: Vec<(String, ColumnType, String)>,
    set: Vec<(String, ColumnType, String)>,
) -> Result<QueryOutput> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let pk = parse_cells(&pk)?;
    let set = parse_cells(&set)?;
    let sql = dby_core::edit::build_update(driver.dialect(), &table, &pk, &set);

    let started = Utc::now();
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    let result = execute_buffered(
        active.conn.as_mut(),
        database.as_deref(),
        &sql,
        &ExecOpts::default(),
    )
    .await;

    let duration_ms = (Utc::now() - started).num_milliseconds().max(0) as u64;
    let mut rec = ExecutionRecord::new(project_id, sql, SqlOrigin::DataEdit);
    rec.connection_id = Some(id.to_string());
    rec.connection_name = Some(conn_name);
    rec.database = database.clone();
    rec.duration_ms = duration_ms;
    match result {
        Ok(output) => {
            rec.status = "ok".to_string();
            rec.rows_affected = output.affected_rows;
            rec.row_count = output.first_result_set().map(|rs| rs.rows.len() as u64);
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Ok(output)
        }
        Err(e) => {
            rec.status = e.to_string();
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Err(e)
        }
    }
}

// ---------- DDL（Schema 管理） ----------

async fn driver_for(state: &Arc<AppState>, id: u64) -> Result<Arc<dyn Driver>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let driver_id = entry.lock().await.driver_id.clone();
    state.registry.resolve(&driver_id)
}

async fn run_ddl(
    state: &Arc<AppState>,
    id: u64,
    database: Option<String>,
    sql: String,
) -> Result<QueryOutput> {
    let started = Utc::now();
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state, &mut active).await?;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    let result = execute_buffered(
        active.conn.as_mut(),
        database.as_deref(),
        &sql,
        &ExecOpts::default(),
    )
    .await;

    let duration_ms = (Utc::now() - started).num_milliseconds().max(0) as u64;
    let mut rec = ExecutionRecord::new(project_id, sql, SqlOrigin::SchemaEdit);
    rec.connection_id = Some(id.to_string());
    rec.connection_name = Some(conn_name);
    rec.database = database.clone();
    rec.duration_ms = duration_ms;
    match result {
        Ok(o) => {
            rec.status = "ok".to_string();
            rec.rows_affected = o.affected_rows;
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Ok(o)
        }
        Err(e) => {
            rec.status = e.to_string();
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn create_database(
    state: State<'_, Arc<AppState>>,
    id: u64,
    name: String,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_create_database(driver.dialect(), &name);
    run_ddl(state.inner(), id, None, sql).await
}

#[tauri::command]
pub async fn drop_database(
    state: State<'_, Arc<AppState>>,
    id: u64,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_database(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, None, sql).await
}

#[tauri::command]
pub async fn create_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    columns: Vec<ColumnDef>,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_create_table(driver.dialect(), &name, &columns);
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn rename_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    old_name: String,
    new_name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_rename_table(driver.dialect(), &old_name, &new_name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn drop_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_table(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn drop_view(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_view(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn drop_routine(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    kind: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_routine(driver.dialect(), &kind, &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn drop_trigger(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_trigger(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn truncate_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_truncate_table(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

// ---------- 导出 ----------

#[tauri::command]
pub async fn export_result(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: Option<String>,
    sql: String,
    format: String,
    table: Option<String>,
    confirmed: bool,
) -> Result<String> {
    guard_dangerous(&sql, confirmed)?;
    let started = Utc::now();
    // 查询实例 token：注册到全局注册表（Arc 共享内部状态），RAII 守卫保证任何退出路径（含 `?` 早退）自动注销（#23）。
    let query_id = uuid::Uuid::new_v4().to_string();
    let key = format!("{id}:{query_id}");
    let token = CancellationToken::new();
    state
        .query_tokens
        .lock()
        .unwrap()
        .insert(key.clone(), Arc::new(token.clone()));
    let _guard = QueryTokenGuard {
        map: state.query_tokens.clone(),
        key,
    };
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    let driver_id = active.driver_id.clone();
    let output = execute_buffered(
        active.conn.as_mut(),
        database.as_deref(),
        &sql,
        &ExecOpts {
            cancel: Some(token),
            ..Default::default()
        },
    )
    .await;

    // 记录历史（origin=export）
    let duration_ms = (Utc::now() - started).num_milliseconds().max(0) as u64;
    let mut rec = ExecutionRecord::new(project_id, sql.clone(), SqlOrigin::Export);
    rec.connection_id = Some(id.to_string());
    rec.connection_name = Some(conn_name);
    rec.database = database.clone();
    rec.duration_ms = duration_ms;
    let output = match output {
        Ok(o) => {
            rec.status = "ok".to_string();
            rec.rows_affected = o.affected_rows;
            rec.row_count = o.first_result_set().map(|rs| rs.rows.len() as u64);
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            o
        }
        Err(e) => {
            // 秒断：取消即关 socket，连接毒化 → 标记重连；历史 status=cancelled（#5）。
            if matches!(e, DbError::Cancelled) {
                active.needs_reconnect = true;
                rec.status = "cancelled".to_string();
            } else {
                rec.status = e.to_string();
            }
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            return Err(e);
        }
    };

    let rs = output
        .first_result_set()
        .ok_or_else(|| DbError::Database("无结果集可导出".to_string()))?;
    let driver = state.registry.resolve(&driver_id)?;
    let text = match format.as_str() {
        "csv" => dby_core::export::to_csv(rs),
        "json" => dby_core::export::to_json(rs),
        "markdown" => dby_core::export::to_markdown(rs),
        "insert" => {
            let t = table.ok_or_else(|| DbError::Config("INSERT 导出需要表名".to_string()))?;
            dby_core::export::to_insert_sql(driver.dialect(), &t, rs)
        }
        other => return Err(DbError::Config(format!("未知导出格式: {other}"))),
    };
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 脱敏视图序列化后不得出现任何 secret 字段/值（#22）。
    #[test]
    fn saved_view_has_no_secret() {
        let v = SavedConnectionView {
            id: "c1".to_string(),
            project_id: "p1".to_string(),
            name: "demo".to_string(),
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            database: Some("app".to_string()),
            has_ssh: true,
            ssh_host: Some("10.0.0.1".to_string()),
            ssh_port: Some(22),
            ssh_user: Some("ubuntu".to_string()),
            color: Some("#1e90ff".to_string()),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(!json.contains("password"), "JSON 不得包含 password 字段");
        assert!(
            !json.contains("private_key"),
            "JSON 不得包含 private_key 字段"
        );
    }

    #[test]
    fn guard_rejects_dangerous_without_confirm() {
        assert!(guard_dangerous("DROP TABLE t", false).is_err());
        assert!(guard_dangerous("DROP TABLE t", true).is_ok());
        assert!(guard_dangerous("SELECT 1", false).is_ok());
        assert!(guard_dangerous("UPDATE t SET x=1", false).is_ok()); // Warn 非 Dangerous，走前端提示
    }

    /// `save=false` 时必须完全跳过持久化：不写 config、不写 keyring（#6/#43）。
    /// keyring 无法直接 mock，故注入 fake `store`/`delete`，断言二者均不被调用。
    #[tokio::test]
    async fn connect_save_false_writes_nothing() {
        let state = Arc::new(AppState::new(
            dby_core::config::AppConfig::with_default_project(),
            dby_core::history::HistoryStore::open_in_memory().unwrap(),
            std::path::PathBuf::from("unused-config.json"),
        ));
        let params = ConnectParams {
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: Some("mysql-pw".to_string()),
            database: None,
            ssl: None,
            ssh: Some(dby_core::driver::SshOptions {
                enabled: true,
                host: "ssh.example.com".to_string(),
                user: "ubuntu".to_string(),
                password: Some("ssh-pw".to_string()),
                private_key: Some("ssh-key".to_string()),
                ..Default::default()
            }),
            params: std::collections::HashMap::new(),
        };
        let config = ConnectionConfig {
            id: "cfg-1".to_string(),
            project_id: "p1".to_string(),
            name: "demo".to_string(),
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            database: None,
            ssl: None,
            ssh: None,
            color: None,
            params: std::collections::HashMap::new(),
        };

        let result = persist_connection(
            &state,
            42,
            config,
            &params,
            false, // save=false：不持久化
            true,  // remember_password 无意义（save=false 短路）
            SecretsIo {
                store: |_, _| panic!("save=false 时不得调用 keyring 写入"),
                delete: |_| panic!("save=false 时不得调用 keyring 删除"),
            },
        )
        .await;

        assert!(result.is_ok(), "save=false 应直接成功: {result:?}");
        assert!(
            state.config.lock().await.connections.is_empty(),
            "save=false 不得写入 config"
        );
        assert!(
            state.connections.lock().unwrap().is_empty(),
            "save=false 不得产生活跃连接残留"
        );
    }

    /// `build_params_from_config`：ssh.password/private_key 必须来自 secrets（而非 config 中残留明文），
    /// params 必须回填，MySQL 密码来自 secrets（#22/#63）。
    #[test]
    fn build_params_from_config_ssh_secrets_override_config() {
        let config = ConnectionConfig {
            id: "c1".to_string(),
            project_id: "p1".to_string(),
            name: "demo".to_string(),
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            database: Some("app".to_string()),
            ssl: None,
            ssh: Some(dby_core::driver::SshOptions {
                enabled: true,
                host: "10.0.0.1".to_string(),
                user: "ubuntu".to_string(),
                password: Some("config-明文".to_string()), // 不得被使用
                private_key: Some("config-明文".to_string()), // 不得被使用
                ..Default::default()
            }),
            color: None,
            params: std::collections::HashMap::from([(
                "charset".to_string(),
                "utf8mb4".to_string(),
            )]),
        };
        let secrets = SshSecrets {
            password: Some("mysql-pw".to_string()),
            ssh_password: Some("ssh-pw".to_string()),
            ssh_private_key: Some("ssh-key".to_string()),
        };

        let params = build_params_from_config(&config, &secrets);

        assert_eq!(params.driver, "mysql");
        assert_eq!(params.host, "127.0.0.1");
        assert_eq!(params.port, 3306);
        assert_eq!(params.user, "root");
        assert_eq!(params.password.as_deref(), Some("mysql-pw"));
        assert_eq!(params.database.as_deref(), Some("app"));
        let ssh = params.ssh.expect("ssh 应保留");
        assert_eq!(
            ssh.password.as_deref(),
            Some("ssh-pw"),
            "ssh.password 必须来自 secrets"
        );
        assert_eq!(
            ssh.private_key.as_deref(),
            Some("ssh-key"),
            "ssh.private_key 必须来自 secrets"
        );
        assert_eq!(ssh.host, "10.0.0.1");
        assert_eq!(ssh.user, "ubuntu");
        assert_eq!(
            params.params.get("charset").map(String::as_str),
            Some("utf8mb4"),
            "params 必须回填"
        );
    }

    /// R1（design §4.6，覆盖 brief「删除后级联清理」措辞）：`delete_project` 必须 REJECT（不级联）——
    /// 项目下存在「已保存连接」（config.connections，非仅活跃连接）时返回 `DbError::Config`，
    /// 且项目与已保存连接均须保留（#41）。
    #[tokio::test]
    async fn delete_project_rejects_or_cascades_saved_connections() {
        let mut config = dby_core::config::AppConfig::with_default_project();
        let project_id = config.projects[0].id.clone();
        config.connections.push(ConnectionConfig {
            id: "cfg-saved-1".to_string(),
            project_id: project_id.clone(),
            name: "saved".to_string(),
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            database: None,
            ssl: None,
            ssh: None,
            color: None,
            params: std::collections::HashMap::new(),
        });
        let state = Arc::new(AppState::new(
            config,
            dby_core::history::HistoryStore::open_in_memory().unwrap(),
            std::path::PathBuf::from("unused-config.json"),
        ));

        let result = delete_project_impl(&state, &project_id).await;

        assert!(
            result.is_err(),
            "项目下存在已保存连接时必须拒绝删除（不级联）"
        );
        assert!(
            matches!(result, Err(DbError::Config(_))),
            "必须返回 DbError::Config"
        );
        let cfg = state.config.lock().await;
        assert!(
            cfg.projects.iter().any(|p| p.id == project_id),
            "拒绝后项目必须保留（REJECT 而非级联删除）"
        );
        assert!(
            cfg.connections.iter().any(|c| c.id == "cfg-saved-1"),
            "拒绝后已保存连接必须保留"
        );
    }

    /// `apply_update`：把 `UpdateSavedConnection` 的 Some 字段映射到 config（#63）。
    /// name/color 有值则覆盖；ssh=None 时保留原 ssh（不清空）；id 绝不被改写。
    #[test]
    fn update_saved_connection_changes_name_and_color() {
        let mut config = ConnectionConfig {
            id: "c1".to_string(),
            project_id: "p1".to_string(),
            name: "old-name".to_string(),
            driver: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            database: None,
            ssl: None,
            ssh: Some(dby_core::driver::SshOptions {
                enabled: true,
                host: "10.0.0.1".to_string(),
                port: 22,
                user: "ubuntu".to_string(),
                ..Default::default()
            }),
            color: None,
            params: std::collections::HashMap::new(),
        };
        let update = UpdateSavedConnection {
            name: Some("new-name".to_string()),
            color: Some("#ff0000".to_string()),
            ssh: None,
        };

        apply_update(&mut config, &update);

        assert_eq!(config.name, "new-name");
        assert_eq!(config.color.as_deref(), Some("#ff0000"));
        let ssh = config.ssh.as_ref().expect("ssh 应保留");
        assert_eq!(ssh.host, "10.0.0.1");
        assert_eq!(config.id, "c1", "id 不得被改写");
    }

    /// 无 SSH 配置时，即使 secrets 为空也不得凭空产生 ssh/密码。
    #[test]
    fn build_params_from_config_no_ssh_keeps_none() {
        let config = ConnectionConfig {
            id: "c2".to_string(),
            project_id: "p1".to_string(),
            name: "plain".to_string(),
            driver: "mysql".to_string(),
            host: "db.internal".to_string(),
            port: 3306,
            user: "app".to_string(),
            database: None,
            ssl: None,
            ssh: None,
            color: None,
            params: std::collections::HashMap::new(),
        };

        let params = build_params_from_config(&config, &SshSecrets::default());

        assert!(params.ssh.is_none(), "无 SSH 配置时不得产生 ssh");
        assert!(params.password.is_none(), "无 secrets 时密码应为 None");
        assert_eq!(params.host, "db.internal");
        assert!(params.params.is_empty());
    }

    /// `parse_cells`：`(列名, 列类型, 输入串)` 按列类型解析为 `Value`，
    /// 失败传播 `parse_value` 的 `DbError::Other`（#11/#69）。
    #[test]
    fn parse_cells_parses_by_column_type() {
        use dby_core::value::Value;

        let cells = vec![
            (
                "id".to_string(),
                ColumnType {
                    base: dby_core::metadata::ColumnTypeBase::I64,
                    ..Default::default()
                },
                "42".to_string(),
            ),
            (
                "name".to_string(),
                ColumnType {
                    base: dby_core::metadata::ColumnTypeBase::Str,
                    ..Default::default()
                },
                "hi".to_string(),
            ),
            (
                "price".to_string(),
                ColumnType {
                    base: dby_core::metadata::ColumnTypeBase::Decimal,
                    ..Default::default()
                },
                "1.50".to_string(),
            ),
        ];

        let parsed = parse_cells(&cells).unwrap();
        assert_eq!(
            parsed,
            vec![
                ("id".to_string(), Value::I64(42)),
                ("name".to_string(), Value::Str("hi".to_string())),
                ("price".to_string(), Value::Decimal("1.50".to_string())),
            ]
        );

        // 非法输入：按列类型解析失败，错误传播（不产出正则猜测的 Str）
        let bad = vec![(
            "id".to_string(),
            ColumnType {
                base: dby_core::metadata::ColumnTypeBase::I64,
                ..Default::default()
            },
            "abc".to_string(),
        )];
        let err = parse_cells(&bad).unwrap_err();
        assert!(err.to_string().contains("无法将 'abc' 解析为 i64"));
    }

    /// `cancel_query` 只取消前缀 `"{id}:"` 匹配的查询实例 token，其它连接的 token 不受影响（#23）。
    /// 直接测抽出的注册表取消逻辑（Tauri `State` 无法在测试中构造，行为等价于 `cancel_query`）。
    #[tokio::test]
    async fn cancel_query_hits_only_matching_connection() {
        let state = Arc::new(AppState::new(
            dby_core::config::AppConfig::with_default_project(),
            dby_core::history::HistoryStore::open_in_memory().unwrap(),
            std::path::PathBuf::from("unused-config.json"),
        ));
        let t1 = Arc::new(dby_core::query::CancellationToken::new());
        let t2 = Arc::new(dby_core::query::CancellationToken::new());
        {
            let mut tokens = state.query_tokens.lock().unwrap();
            tokens.insert("1:q1".to_string(), t1.clone());
            tokens.insert("2:q2".to_string(), t2.clone());
        }

        cancel_queries_for_connection(&state.query_tokens, 1);

        assert!(t1.is_cancelled(), "前缀 \"1:\" 的查询 token 必须被取消");
        assert!(
            !t2.is_cancelled(),
            "其它连接（\"2:q2\"）的 token 不得被取消"
        );
    }

    /// `ChannelSink.on_event` 在 channel 已关闭（send 失败）时必须触发取消（#42）：
    /// 前端关标签后 channel 失效，后端若继续执行就成了无主孤儿。
    /// Tauri Channel 无法在测试中直接「关闭」，但 `Channel::new` 接受自定义 on_message，
    /// 可注入必然失败的路径（design §6「可注入失败路径的抽象」），等价于前端已关闭 channel。
    /// NOTE: 本机 shell 测试二进制无法运行（0xC0000139），运行时 RED/GREEN 延后验证。
    #[test]
    fn channel_sink_cancels_on_send_failure() {
        let cancel = dby_core::query::CancellationToken::new();
        let channel = Channel::<StreamEvent>::new(|_| {
            Err(tauri::Error::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "channel closed",
            )))
        });
        let mut sink = ChannelSink {
            channel,
            rows: 0,
            cancel: cancel.clone(),
        };

        sink.on_event(StreamEvent::Rows(Vec::new()));

        assert!(
            cancel.is_cancelled(),
            "channel send 失败必须触发取消（#42）"
        );
    }
}
