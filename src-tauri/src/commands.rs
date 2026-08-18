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
use dby_core::metadata::{ColumnInfo, ColumnType, TableInfo};
use dby_core::project::Project;
use dby_core::query::{ExecOpts, QueryOutput, ResultSink, SqlOrigin, StreamEvent};
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
}

#[derive(Serialize)]
pub struct ConnectionSummary {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
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
    }
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
        project_id,
        name,
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
    persist_connection(
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
    Ok(resp)
}

/// 钥匙串读写单元：`store_secrets` / `delete_secrets` 的注入点。
/// `keyring::Entry` 无法直接 mock，单测传入 fake 以断言「save=false 不写」。
struct SecretsIo {
    store: fn(&str, &ConnectParams) -> Result<()>,
    delete: fn(&str),
}

/// 连接成功后的持久化控制流（#6/#43）：
///
/// - `save=false`：不写 config、不写 keyring（no-op）。
/// - `save=true`：secrets 先存（`remember_password` 时）、config 后存；任一步失败都回滚已写内容并断开会话，不留半失败态。
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
) -> Result<()> {
    if !save {
        return Ok(());
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
    Ok(())
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

    let secrets = SshSecrets {
        password: get_secret(&secret_key(&config_id, SecretKind::MysqlPassword)).ok(),
        ssh_password: get_secret(&secret_key(&config_id, SecretKind::SshPassword)).ok(),
        ssh_private_key: get_secret(&secret_key(&config_id, SecretKind::SshPrivateKey)).ok(),
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
    open_session(
        state.inner(),
        &params,
        config.project_id.clone(),
        config.name.clone(),
    )
    .await
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
    active.conn.columns(&database, &table).await
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
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
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
            rec.status = e.to_string();
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
}

impl ResultSink for ChannelSink {
    fn on_event(&mut self, ev: StreamEvent) {
        if let StreamEvent::Rows(ref rows) = ev {
            self.rows += rows.len();
        }
        if let Err(e) = self.channel.send(ev) {
            log::warn!("转发流式事件到前端失败: {e}");
        }
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
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    // Task 4 引入查询实例 token 后在此注入 `ExecOpts.cancel`（#23）；当前取消能力由 Task 4 恢复。
    let opts = ExecOpts::default();
    let mut sink = ChannelSink { channel, rows: 0 };
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
            rec.status = "ok".to_string();
            if let Err(e) = state.history.record(&rec) {
                log::warn!("写入历史记录失败: {e}");
            }
            Ok(())
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
pub async fn cancel_query(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    // 只读查询实例 token 注册表（不抢连接锁，#21/#23）；Task 4 填充 token，当前为空。
    let tokens = state.query_tokens.lock().unwrap();
    let prefix = format!("{id}:");
    for (k, t) in tokens.iter() {
        if k.starts_with(&prefix) {
            t.cancel();
        }
    }
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
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;
    let project_id = active.project_id.clone();
    let conn_name = active.name.clone();
    let driver_id = active.driver_id.clone();
    let output = execute_buffered(
        active.conn.as_mut(),
        database.as_deref(),
        &sql,
        &ExecOpts::default(),
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
            rec.status = e.to_string();
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
}
