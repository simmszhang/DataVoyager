use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tauri::State;

use dby_core::danger::DangerLevel;
use dby_core::driver::{execute_buffered, ConnectParams, DriverInfo};
use dby_core::error::{DbError, Result};
use dby_core::history::{ExecutionRecord, HistoryFilter, StatementHit};
use dby_core::metadata::{ColumnInfo, TableInfo};
use dby_core::project::Project;
use dby_core::query::{ExecOpts, QueryOutput, ResultSink, SqlOrigin, StreamEvent};
use tauri::ipc::Channel;

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
    let driver = state.registry.resolve(&params.driver)?;
    let conn = driver.connect(&params).await?;
    Ok(conn.server_version())
}

#[tauri::command]
pub async fn connect(
    state: State<'_, Arc<AppState>>,
    params: ConnectParams,
    project_id: Option<String>,
) -> Result<ConnectResponse> {
    let driver = state.registry.resolve(&params.driver)?;
    let conn = driver.connect(&params).await?;
    let server_version = conn.server_version();
    let database = params.database.clone().unwrap_or_default();
    let project_id = state.resolve_project_id(project_id).await;
    let name = format!(
        "{}@{}:{}{}",
        params.user,
        params.host,
        params.port,
        if database.is_empty() {
            String::new()
        } else {
            format!("/{database}")
        }
    );
    let id = state.alloc_id();
    let active = ActiveConnection {
        id,
        name: name.clone(),
        driver_id: driver.id().to_string(),
        project_id: project_id.clone(),
        database: database.clone(),
        server_version: server_version.clone(),
        cancel: dby_core::query::CancellationToken::new(),
        conn,
    };
    state.connections.lock().await.insert(id, active);
    Ok(ConnectResponse {
        id,
        name,
        driver_id: driver.id().to_string(),
        project_id,
        database,
        server_version,
    })
}

#[tauri::command]
pub async fn disconnect(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    state.connections.lock().await.remove(&id);
    Ok(())
}

#[tauri::command]
pub async fn list_connections(state: State<'_, Arc<AppState>>) -> Result<Vec<ConnectionSummary>> {
    let guard = state.connections.lock().await;
    let mut v: Vec<ConnectionSummary> = guard.values().map(snapshot).collect();
    v.sort_by_key(|c| c.id);
    Ok(v)
}

#[tauri::command]
pub async fn list_databases(state: State<'_, Arc<AppState>>, id: u64) -> Result<Vec<String>> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.schemas(None).await
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<TableInfo>> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.tables(&database).await
}

#[tauri::command]
pub async fn list_columns(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<ColumnInfo>> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.columns(&database, &table).await
}

#[tauri::command]
pub async fn execute_query(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: Option<String>,
    sql: String,
) -> Result<QueryOutput> {
    let started = Utc::now();
    let (project_id, conn_name, result) = {
        let mut guard = state.connections.lock().await;
        let active = guard
            .get_mut(&id)
            .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
        let project_id = active.project_id.clone();
        let conn_name = active.name.clone();
        let result = execute_buffered(
            active.conn.as_mut(),
            database.as_deref(),
            &sql,
            &ExecOpts::default(),
        )
        .await;
        (project_id, conn_name, result)
    };

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
            let _ = state.history.record(&rec);
            Ok(output)
        }
        Err(e) => {
            rec.status = e.to_string();
            let _ = state.history.record(&rec);
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
pub async fn delete_project(state: State<'_, Arc<AppState>>, id: String) -> Result<()> {
    {
        let guard = state.connections.lock().await;
        if guard.values().any(|c| c.project_id == id) {
            return Err(DbError::Config("项目下仍有连接，请先删除连接".to_string()));
        }
    }
    let mut cfg = state.config.lock().await;
    cfg.projects.retain(|p| p.id != id);
    cfg.save(&state.config_path)?;
    state.history.clear(Some(&id))?;
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
        let _ = self.channel.send(ev);
    }
}

#[tauri::command]
pub async fn execute_query_stream(
    state: State<'_, Arc<AppState>>,
    channel: Channel<StreamEvent>,
    id: u64,
    database: Option<String>,
    sql: String,
) -> Result<()> {
    let started = Utc::now();
    let (project_id, conn_name, result, row_count) = {
        let mut guard = state.connections.lock().await;
        let active = guard
            .get_mut(&id)
            .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
        let project_id = active.project_id.clone();
        let conn_name = active.name.clone();
        let cancel = active.cancel.clone();
        let opts = ExecOpts {
            cancel: Some(cancel),
            ..Default::default()
        };
        let mut sink = ChannelSink {
            channel,
            rows: 0,
        };
        let result = active
            .conn
            .execute_stream(database.as_deref(), &sql, &opts, &mut sink)
            .await;
        (project_id, conn_name, result, sink.rows as u64)
    };

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
            let _ = state.history.record(&rec);
            Ok(())
        }
        Err(e) => {
            rec.status = e.to_string();
            let _ = state.history.record(&rec);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn cancel_query(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let guard = state.connections.lock().await;
    if let Some(active) = guard.get(&id) {
        active.cancel.cancel();
    }
    Ok(())
}

// ---------- 危险操作分析 ----------

#[tauri::command]
pub fn analyze_danger(sql: String) -> DangerLevel {
    dby_core::danger::analyze_danger(&sql)
}

// ---------- 事务 ----------

#[tauri::command]
pub async fn begin(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.begin().await
}

#[tauri::command]
pub async fn commit(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.commit().await
}

#[tauri::command]
pub async fn rollback(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.rollback().await
}

#[tauri::command]
pub async fn set_autocommit(state: State<'_, Arc<AppState>>, id: u64, enabled: bool) -> Result<()> {
    let mut guard = state.connections.lock().await;
    let active = guard
        .get_mut(&id)
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    active.conn.set_autocommit(enabled).await
}
