use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tauri::State;

use dby_core::driver::{ConnectParams, DriverInfo};
use dby_core::error::{DbError, Result};
use dby_core::history::{ExecutionRecord, HistoryFilter, StatementHit};
use dby_core::metadata::{ColumnInfo, TableInfo};
use dby_core::project::Project;
use dby_core::query::{ExecOpts, QueryOutput, SqlOrigin};

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
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let driver = state.registry.resolve(&params.driver)?;
        let conn = driver.connect(&params)?;
        Ok(conn.server_version())
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn connect(
    state: State<'_, Arc<AppState>>,
    params: ConnectParams,
    project_id: Option<String>,
) -> Result<ConnectResponse> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let driver = state.registry.resolve(&params.driver)?;
        let conn = driver.connect(&params)?;
        let server_version = conn.server_version();
        let database = params.database.clone().unwrap_or_default();
        let project_id = state.resolve_project_id(project_id);
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
            conn,
        };
        state
            .connections
            .lock()
            .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?
            .insert(id, active);
        Ok(ConnectResponse {
            id,
            name,
            driver_id: driver.id().to_string(),
            project_id,
            database,
            server_version,
        })
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn disconnect(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        state
            .connections
            .lock()
            .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?
            .remove(&id);
        Ok(())
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn list_connections(state: State<'_, Arc<AppState>>) -> Result<Vec<ConnectionSummary>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let guard = state
            .connections
            .lock()
            .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?;
        let mut v: Vec<ConnectionSummary> = guard.values().map(snapshot).collect();
        v.sort_by_key(|c| c.id);
        Ok(v)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn list_databases(state: State<'_, Arc<AppState>>, id: u64) -> Result<Vec<String>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state
            .connections
            .lock()
            .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?;
        let active = guard
            .get_mut(&id)
            .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
        active.conn.schemas(None)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
) -> Result<Vec<TableInfo>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state
            .connections
            .lock()
            .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?;
        let active = guard
            .get_mut(&id)
            .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
        active.conn.tables(&database)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn list_columns(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<ColumnInfo>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state
            .connections
            .lock()
            .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?;
        let active = guard
            .get_mut(&id)
            .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
        active.conn.columns(&database, &table)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn execute_query(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: Option<String>,
    sql: String,
) -> Result<QueryOutput> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let started = Utc::now();
        let (project_id, conn_name, result) = {
            let mut guard = state
                .connections
                .lock()
                .map_err(|_| DbError::Other("connections lock poisoned".to_string()))?;
            let active = guard
                .get_mut(&id)
                .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
            let project_id = active.project_id.clone();
            let conn_name = active.name.clone();
            let result = active.conn.execute(database.as_deref(), &sql, &ExecOpts::default());
            (project_id, conn_name, result)
        };

        // 统一历史捕获：手动与工具生成的 SQL 都经引擎 execute，在此归因。
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
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

// ---------- 项目 ----------

#[tauri::command]
pub fn list_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<Project>> {
    let cfg = state
        .config
        .lock()
        .map_err(|_| DbError::Other("config lock poisoned".to_string()))?;
    Ok(cfg.projects.clone())
}

#[tauri::command]
pub fn create_project(state: State<'_, Arc<AppState>>, name: String) -> Result<Project> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|_| DbError::Other("config lock poisoned".to_string()))?;
    let project = Project::new(name);
    cfg.projects.push(project.clone());
    cfg.save(&state.config_path)?;
    Ok(project)
}

// ---------- 历史 ----------

#[tauri::command]
pub async fn search_history(
    state: State<'_, Arc<AppState>>,
    query: String,
    project_id: Option<String>,
) -> Result<Vec<StatementHit>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut filter = HistoryFilter::new();
        filter.project_id = project_id;
        state.history.search(&query, &filter)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn list_history(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
) -> Result<Vec<StatementHit>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut filter = HistoryFilter::new();
        filter.project_id = project_id;
        state.history.statements(&filter)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}

#[tauri::command]
pub async fn list_executions(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
) -> Result<Vec<ExecutionRecord>> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut filter = HistoryFilter::new();
        filter.project_id = project_id;
        state.history.executions(&filter)
    })
    .await
    .map_err(|e| DbError::Other(format!("task failed: {e}")))?
}
