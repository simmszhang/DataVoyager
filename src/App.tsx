import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Channel } from "@tauri-apps/api/core";
import {
  api,
  displayCell,
  DriverInfo,
  EditCell,
  SavedConnection,
  StreamEvent,
  UNKNOWN_COLUMN_TYPE,
} from "./api";
import { useStore } from "./store";
import { errToString } from "./i18n";
import ConnectionDialog from "./components/ConnectionDialog";
import SchemaTree from "./components/SchemaTree";
import TableStructureEditor from "./components/TableStructureEditor";
import QueryEditor from "./components/QueryEditor";
import ResultsGrid from "./components/ResultsGrid";
import ExportDialog from "./components/ExportDialog";
import HistoryPanel from "./components/HistoryPanel";
import "./App.css";

export default function App() {
  const {
    projects,
    projectId,
    connections,
    activeId,
    tabs,
    workspaces,
    setProjects,
    setProjectId,
    setConnections,
    openConnection,
    closeConnection,
    setActive,
    updateWorkspace,
    mutateResult,
  } = useStore();

  const { t } = useTranslation();

  const [drivers, setDrivers] = useState<DriverInfo[]>([]);
  const [savedConnections, setSavedConnections] = useState<SavedConnection[]>([]);
  const [showDialog, setShowDialog] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [pendingDanger, setPendingDanger] = useState<{
    sql: string;
    reasons: string[];
  } | null>(null);
  const [pendingWarn, setPendingWarn] = useState<{ sql: string } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [pendingEdit, setPendingEdit] = useState<{
    sql: string;
    table: string;
    database: string | null;
    pk: EditCell[];
    set: EditCell[];
  } | null>(null);
  const [structureEditor, setStructureEditor] = useState<{
    connId: number;
    database: string;
    table: string;
  } | null>(null);

  const activeConn = connections.find((c) => c.id === activeId) ?? null;
  const visibleConnections = connections.filter((c) => c.project_id === projectId);
  
  // Merge active connections with saved connections for display (#71)
  // Saved connections without matching active connection are shown as disconnected
  const displayConnections = [
    ...visibleConnections,
    ...savedConnections
      .filter((saved) => saved.project_id === projectId)
      .filter((saved) => !visibleConnections.some((active) => active.config_id === saved.id))
      .map((saved) => ({
        id: -1, // Placeholder ID for disconnected saved connections
        name: saved.name,
        driver_id: saved.driver,
        project_id: saved.project_id,
        database: saved.database || "",
        server_version: "",
        config_id: saved.id,
      })),
  ];
  
  const ws = activeId != null ? workspaces[activeId] : undefined;

  useEffect(() => {
    api.listDrivers().then(setDrivers).catch(() => {});
    api
      .listProjects()
      .then((ps) => {
        setProjects(ps);
        if (ps.length > 0) {
          setProjectId(ps[0].id);
          loadSavedConnections(ps[0].id);
        }
      })
      .catch(() => {});
    // Also load active connections on mount (#53)
    api.listConnections().then(setConnections).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Load saved connections for a project (#71)
  async function loadSavedConnections(projectId: string) {
    try {
      const saved = await api.listSavedConnections(projectId);
      setSavedConnections(saved);
    } catch (e) {
      console.error("Failed to load saved connections:", e);
    }
  }

  /// 连接成功后：刷新连接列表 + 打开标签。
  async function finishConnect() {
    const list = await api.listConnections();
    setConnections(list);
    const newest = list[list.length - 1];
    if (!newest) return;
    openConnection(newest);
    setStatus(t("app.status.connected", { name: newest.name, version: newest.server_version }));
    if (newest.database) {
      updateWorkspace(newest.id, { selectedDb: newest.database });
    }
    // Reload saved connections after new connection (#71)
    if (projectId) {
      loadSavedConnections(projectId);
    }
  }

  async function handleConnected() {
    setShowDialog(false);
    try {
      await finishConnect();
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  function handleSelectConnection(id: number) {
    // #74: 占位符连接（id === -1）是未激活的保存连接，需要重连
    if (id === -1) {
      // 从 displayConnections 中找到对应的保存连接并重连
      const savedConn = displayConnections.find((c) => c.id === id);
      if (savedConn?.config_id) {
        handleReconnect(savedConn.config_id);
      }
      return;
    }
    const conn = connections.find((c) => c.id === id);
    if (conn) openConnection(conn);
  }

  /// 树节点点击表：设置该连接的默认库 + 编辑区查询。
  /// SQL 由后端按连接方言生成（#4），前端不再硬编码反引号/LIMIT。
  async function handleOpenTable(connId: number, database: string, table: string) {
    updateWorkspace(connId, {
      selectedDb: database,
      selectedTable: table,
    });
    try {
      const sql = await api.buildTableSelect(connId, table);
      updateWorkspace(connId, { query: sql });
    } catch (e) {
      updateWorkspace(connId, { error: errToString(e) });
      setStatus(t("app.status.tableSqlFailed"));
    }
  }

  async function handleShowDDL(connId: number, database: string, table: string) {
    try {
      const ddl = await api.showCreateTable(connId, database, table);
      updateWorkspace(connId, { query: ddl });
      setStatus(t("app.status.ddlLoaded"));
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  function handleEditStructure(connId: number, database: string, table: string) {
    setStructureEditor({ connId, database, table });
  }

  // #75: 插入 DDL 模板到编辑器
  function handleInsertTemplate(connId: number, template: string) {
    updateWorkspace(connId, { query: template });
    setStatus(t("app.status.templateInserted"));
  }

  async function runQuery(id: number, sql: string, confirmed: boolean = false) {
    const db = workspaces[id]?.selectedDb ?? "";
    updateWorkspace(id, {
      running: true,
      error: null,
      result: {
        columns: null,
        rows: [],
        affected_rows: 0,
        last_insert_id: null,
        truncated: false,
      },
    });
    const channel = new Channel<StreamEvent>();
    channel.onmessage = (ev) => {
      switch (ev.event) {
        case "columns":
          mutateResult(id, (r) => {
            r.columns = ev.data;
          });
          break;
        case "rows":
          mutateResult(id, (r) => {
            r.rows.push(...ev.data);
          });
          break;
        case "affected":
          mutateResult(id, (r) => {
            r.affected_rows = ev.data.affected_rows;
            r.last_insert_id = ev.data.last_insert_id;
          });
          break;
        case "info":
          break;
        case "result_set_end":
          // 结果集边界（#28）：按计划暂只展示第一组，多结果集切换属后续任务，此处仅占位。
          break;
        case "truncated":
          // 超行数上限截断：标记 StreamResult.truncated，由 ResultsGrid 落到 UI。
          mutateResult(id, (r) => {
            r.truncated = true;
          });
          break;
        case "done": {
          // 命令成功收尾（S4）：由 channel 终态复位 running，与 invoke 返回解耦；
          // 状态栏计数在此结算（design §4.5），getState 取最新流式结果（闭包可能过期）。
          updateWorkspace(id, { running: false });
          const r = useStore.getState().workspaces[id]?.result;
          if (r) {
            setStatus(
              r.columns
                ? t("app.status.rowsReturned", { count: r.rows.length })
                : t("app.status.rowsAffected", { count: r.affected_rows })
            );
          }
          break;
        }
        case "error": {
          // 命令失败收尾（S4/S5）：kind 区分「取消」与「失败」（#29）——
          // 主动取消（kind==="cancelled"）不闪错误提示，仅复位 running（invoke 拒绝路径兜底）。
          if (ev.data.kind === "cancelled") {
            updateWorkspace(id, { running: false });
          } else {
            updateWorkspace(id, { error: ev.data.message, running: false });
          }
          break;
        }
      }
    };
    try {
      await api.executeQueryStream(channel, id, db || null, sql, confirmed);
      // 状态栏计数已移至 done 事件处理（design §4.5），invoke 返回不再结算。
    } catch (e) {
      const msg = errToString(e);
      // 取消（#5 秒断）：连接已毒化，下次使用自动重连 —— 提示而非报错。
      // running 兜底复位（design §7）：channel 终止事件可能先于 invoke 拒绝到达，重复复位无害。
      const cancelled = msg.includes("cancelled");
      updateWorkspace(id, { error: cancelled ? null : msg, running: false });
      setStatus(cancelled ? t("app.status.cancelledAutoReconnect") : t("app.status.queryFailed"));
    }
  }

  async function handleRun() {
    if (!activeId) return;
    const sql = workspaces[activeId]?.query ?? "SELECT 1";
    try {
      const danger = await api.analyzeDanger(sql);
      if (danger.level === "dangerous") {
        setPendingDanger({ sql, reasons: danger.reasons });
        return;
      }
      if (danger.level === "warn") {
        setPendingWarn({ sql });
        return;
      }
      await runQuery(activeId, sql);
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function handleCancel() {
    if (!activeId) return;
    try {
      await api.cancelQuery(activeId);
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function handleBegin() {
    if (!activeId) return;
    try {
      await api.begin(activeId);
      updateWorkspace(activeId, { inTransaction: true });
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function handleCommit() {
    if (!activeId) return;
    try {
      await api.commit(activeId);
      updateWorkspace(activeId, { inTransaction: false });
      setStatus(t("app.status.committed"));
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function handleRollback() {
    if (!activeId) return;
    try {
      await api.rollback(activeId);
      updateWorkspace(activeId, { inTransaction: false });
      setStatus(t("app.status.rolledBack"));
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function handleToggleAutocommit() {
    if (!activeId) return;
    const next = !(workspaces[activeId]?.autocommit ?? true);
    try {
      await api.setAutocommit(activeId, next);
      updateWorkspace(activeId, { autocommit: next });
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function handleEditCell(rowIndex: number, colIndex: number, newValue: string) {
    if (!activeId || !ws) return;
    const table = ws.selectedTable;
    const rs = ws.result;
    if (!table || !rs || !rs.columns) {
      setStatus(t("app.status.needSelectTable"));
      return;
    }
    const cols = rs.columns;
    const row = rs.rows[rowIndex];
    const pkCols = cols.filter((c) => c.primary_key).map((c) => c.name);
    if (pkCols.length === 0) {
      setStatus(t("app.status.noPkEditable"));
      return;
    }
    // pk 与 set 都提交「列名 + 列类型 + 原始输入串」，由后端 parse_value 按列类型
    // 解析（design §4.6，#11/#69）；主键列同样按类型解析，不再走前端正则猜测。
    const pk: EditCell[] = pkCols.map((name) => {
      const idx = cols.findIndex((c) => c.name === name);
      return [
        name,
        cols[idx].column_type ?? UNKNOWN_COLUMN_TYPE,
        displayCell(row[idx]),
      ];
    });
    const colName = cols[colIndex].name;
    const set: EditCell[] = [
      [colName, cols[colIndex].column_type ?? UNKNOWN_COLUMN_TYPE, newValue],
    ];
    try {
      const sql = await api.buildEditSql(activeId, table, pk, set);
      setPendingEdit({ sql, table, database: ws.selectedDb || null, pk, set });
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function confirmEdit() {
    if (!activeId || !pendingEdit) return;
    const { table, database, pk, set } = pendingEdit;
    setPendingEdit(null);
    try {
      await api.executeEdit(activeId, database, table, pk, set);
      setStatus(t("app.status.updated"));
      const sql = workspaces[activeId]?.query;
      if (sql) runQuery(activeId, sql);
    } catch (e) {
      updateWorkspace(activeId, { error: errToString(e) });
    }
  }

  async function refreshProjects(selectId?: string) {
    const ps = await api.listProjects();
    setProjects(ps);
    if (selectId !== undefined) {
      setProjectId(selectId);
    } else if (ps.length > 0 && !ps.some((p) => p.id === projectId)) {
      setProjectId(ps[0].id);
    }
  }

  async function handleCreateProject() {
    const name = window.prompt(t("app.prompt.projectName"));
    if (!name) return;
    try {
      const p = await api.createProject(name);
      await refreshProjects(p.id);
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  async function handleRenameProject() {
    if (!projectId) return;
    const current = projects.find((p) => p.id === projectId);
    const name = window.prompt(t("app.prompt.renameProject"), current?.name);
    if (!name) return;
    try {
      await api.renameProject(projectId, name);
      await refreshProjects();
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  async function handleDeleteProject() {
    if (!projectId) return;
    if (!window.confirm(t("app.confirm.deleteProject"))) return;
    try {
      await api.deleteProject(projectId, true);
      await refreshProjects();
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  function handleSwitchProject(id: string) {
    setProjectId(id);
    setActive(null);
    loadSavedConnections(id); // Load saved connections for the new project (#71)
  }

  async function handleDisconnect(id: number) {
    try {
      await api.disconnect(id);
      setConnections(connections.filter((c) => c.id !== id));
      closeConnection(id);
      setStatus(null);
      // Reload saved connections after disconnect to show the saved config (#71)
      if (projectId) {
        loadSavedConnections(projectId);
      }
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  // R11: 重连已保存的连接
  async function handleReconnect(configId: string) {
    try {
      const resp = await api.reconnect(configId);
      const list = await api.listConnections();
      setConnections(list);
      const conn = list.find((c) => c.id === resp.id);
      if (conn) {
        openConnection(conn);
        setStatus(t("app.status.connected", { name: resp.name, version: resp.server_version }));
        if (resp.database) {
          updateWorkspace(resp.id, { selectedDb: resp.database });
        }
      }
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  // #73: 删除保存的连接
  async function handleDeleteConnection(configId: string) {
    if (!window.confirm(t("app.confirm.deleteConnection"))) {
      return;
    }
    try {
      await api.deleteSavedConnection(configId);
      setStatus(t("app.status.connectionDeleted"));
      // Reload saved connections list
      if (projectId) {
        loadSavedConnections(projectId);
      }
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  // 快捷键 Ctrl/Cmd+Enter 运行（ref 避免闭包过期）。
  const runRef = useRef<() => void>(() => {});
  runRef.current = handleRun;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
        e.preventDefault();
        runRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app" onContextMenu={(e) => e.preventDefault()}>
      <header className="topbar">
        <div className="brand">dby</div>
        <div className="conn-info">
          {activeConn ? (
            <>
              <span className="conn-name">{activeConn.name}</span>
              <span className="conn-ver">{activeConn.server_version}</span>
              {activeConn.database && <span className="conn-db">{activeConn.database}</span>}
            </>
          ) : (
            <span className="muted">{t("app.status.notConnected")}</span>
          )}
        </div>
        <div className="topbar-actions">
          <button className="btn" onClick={() => setShowDialog(true)}>
            {t("app.action.newConnection")}
          </button>
          {activeConn && (
            <button className="btn" onClick={() => handleDisconnect(activeConn.id)}>
              {t("app.action.disconnect")}
            </button>
          )}
        </div>
      </header>

      <div className="conn-tabs">
        {tabs.map((id) => {
          const c = connections.find((x) => x.id === id);
          if (!c) return null;
          return (
            <div
              key={id}
              className={`conn-tab ${activeId === id ? "active" : ""}`}
              onClick={() => setActive(id)}
            >
              <span className="ellipsis">{c.name}</span>
              <span className="conn-driver">{c.driver_id}</span>
              <button
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  handleDisconnect(id);
                }}
              >
                ×
              </button>
            </div>
          );
        })}
      </div>

      <div className="body">
        <aside className="sidebar">
          <div className="sidebar-head">
            <span className="section-title">{t("app.sidebar.projects")}</span>
            <div className="project-actions">
              <button className="btn small" onClick={handleCreateProject} title={t("app.action.newProject")}>
                +
              </button>
              <button
                className="btn small"
                onClick={handleRenameProject}
                disabled={!projectId}
                title={t("app.action.rename")}
              >
                ✎
              </button>
              <button
                className="btn small"
                onClick={handleDeleteProject}
                disabled={!projectId || projects.length <= 1}
                title={t("app.action.delete")}
              >
                🗑
              </button>
            </div>
          </div>
          <select
            className="project-select"
            value={projectId ?? ""}
            onChange={(e) => handleSwitchProject(e.target.value)}
          >
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>

          <div className="sidebar-head">
            <span className="section-title">{t("app.sidebar.connections")}</span>
            <button className="btn small" onClick={() => setShowDialog(true)}>
              +
            </button>
          </div>
          <SchemaTree
            connections={displayConnections}
            activeId={activeId}
            onSelectConnection={handleSelectConnection}
            onDisconnect={handleDisconnect}
            onReconnect={handleReconnect}
            onDeleteConnection={handleDeleteConnection}
            onOpenTable={handleOpenTable}
            onShowDDL={handleShowDDL}
            onEditStructure={handleEditStructure}
            onInsertTemplate={handleInsertTemplate}
          />
        </aside>

        <main className="main">
          {activeConn && ws ? (
            <>
              <QueryEditor
                value={ws.query}
                running={ws.running}
                onChange={(v) => updateWorkspace(activeId!, { query: v })}
                onRun={handleRun}
                onCancel={handleCancel}
                onExport={() => setShowExport(true)}
                inTransaction={ws.inTransaction}
                autocommit={ws.autocommit}
                onBegin={handleBegin}
                onCommit={handleCommit}
                onRollback={handleRollback}
                onToggleAutocommit={handleToggleAutocommit}
              />
              <section className="results-panel">
                {ws.error ? (
                  <div className="error-box">{ws.error}</div>
                ) : ws.result ? (
                  <ResultsGrid
                    result={ws.result}
                    onEditCell={handleEditCell}
                    tableName={ws.selectedTable}
                    connId={activeId}
                    database={ws.selectedDb}
                    onRefresh={() => {
                      if (activeId && ws.selectedTable && ws.query) {
                        runQuery(activeId, ws.query);
                      }
                    }}
                  />
                ) : (
                  <div className="empty-state">{t("app.empty.selectTableOrRun")}</div>
                )}
              </section>
            </>
          ) : (
            <div className="empty-state">
              <h2>{t("app.welcome.title")}</h2>
              <p>{t("app.welcome.subtitle")}</p>
            </div>
          )}
          {showHistory && (
            <HistoryPanel
              projectId={projectId}
              onLoadSql={(sql) => activeId && updateWorkspace(activeId, { query: sql })}
            />
          )}
        </main>
      </div>

      <footer className="statusbar">
        <button className="btn small" onClick={() => setShowHistory((v) => !v)}>
          {t("app.action.history")}
        </button>
        {ws?.inTransaction && <span className="txn-indicator">{t("app.status.inTransaction")}</span>}
        <span className="status">{status ?? t("app.status.ready")}</span>
        <span className="spacer" />
        <span>
          {t("app.status.counts", {
            drivers: drivers.length,
            projects: projects.length,
            connections: connections.length,
          })}
        </span>
      </footer>

      {showDialog && (
        <ConnectionDialog
          drivers={drivers}
          projectId={projectId}
          onConnected={handleConnected}
          onClose={() => setShowDialog(false)}
        />
      )}

      {showExport && activeId && (
        <ExportDialog
          connectionId={activeId}
          database={ws?.selectedDb || null}
          sql={ws?.query ?? ""}
          onClose={() => setShowExport(false)}
        />
      )}

      {pendingEdit && (
        <div className="modal-backdrop" onClick={() => setPendingEdit(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>{t("app.editConfirm.title")}</h2>
              <button className="icon-btn" onClick={() => setPendingEdit(null)}>
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">{t("app.editConfirm.body")}</p>
              <pre className="danger-sql">{pendingEdit.sql}</pre>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingEdit(null)}>
                {t("app.editConfirm.cancel")}
              </button>
              <button className="btn primary" onClick={confirmEdit}>
                {t("app.editConfirm.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingDanger && (
        <div className="modal-backdrop" onClick={() => setPendingDanger(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>{t("app.dangerConfirm.title")}</h2>
              <button className="icon-btn" onClick={() => setPendingDanger(null)}>
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">{t("app.dangerConfirm.body")}</p>
              <pre className="danger-sql">{pendingDanger.sql}</pre>
              <ul className="danger-reasons">
                {pendingDanger.reasons.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingDanger(null)}>
                {t("app.dangerConfirm.cancel")}
              </button>
              <button
                className="btn danger"
                onClick={() => {
                  const sql = pendingDanger.sql;
                  setPendingDanger(null);
                  if (activeId) runQuery(activeId, sql, true);
                }}
              >
                {t("app.dangerConfirm.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingWarn && (
        <div className="modal-backdrop" onClick={() => setPendingWarn(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>{t("app.warnConfirm.title")}</h2>
              <button className="icon-btn" onClick={() => setPendingWarn(null)}>
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">{t("app.warnConfirm.body")}</p>
              <pre className="danger-sql">{pendingWarn.sql}</pre>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingWarn(null)}>
                {t("app.warnConfirm.cancel")}
              </button>
              <button
                className="btn primary"
                onClick={() => {
                  const sql = pendingWarn.sql;
                  setPendingWarn(null);
                  if (activeId) runQuery(activeId, sql);
                }}
              >
                {t("app.warnConfirm.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {structureEditor && (
        <TableStructureEditor
          connId={structureEditor.connId}
          database={structureEditor.database}
          table={structureEditor.table}
          onClose={() => setStructureEditor(null)}
        />
      )}
    </div>
  );
}
