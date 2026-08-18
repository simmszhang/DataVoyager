import { useEffect, useRef, useState } from "react";
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
import ConnectionDialog from "./components/ConnectionDialog";
import SchemaTree from "./components/SchemaTree";
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

  const [drivers, setDrivers] = useState<DriverInfo[]>([]);
  const [showDialog, setShowDialog] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [pendingDanger, setPendingDanger] = useState<{
    sql: string;
    reasons: string[];
  } | null>(null);
  const [pendingWarn, setPendingWarn] = useState<{ sql: string } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [savedConnections, setSavedConnections] = useState<SavedConnection[]>([]);
  const [pendingEdit, setPendingEdit] = useState<{
    sql: string;
    table: string;
    database: string | null;
    pk: EditCell[];
    set: EditCell[];
  } | null>(null);

  const activeConn = connections.find((c) => c.id === activeId) ?? null;
  const visibleConnections = connections.filter((c) => c.project_id === projectId);
  const ws = activeId != null ? workspaces[activeId] : undefined;

  useEffect(() => {
    api.listDrivers().then(setDrivers).catch(() => {});
    api
      .listProjects()
      .then((ps) => {
        setProjects(ps);
        if (ps.length > 0) setProjectId(ps[0].id);
      })
      .catch(() => {});
    refreshSaved();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function refreshSaved() {
    try {
      setSavedConnections(await api.listSavedConnections(null));
    } catch {
      /* ignore */
    }
  }

  /// 连接成功后：刷新连接列表 + 打开标签。
  async function finishConnect() {
    const list = await api.listConnections();
    setConnections(list);
    const newest = list[list.length - 1];
    if (!newest) return;
    openConnection(newest);
    setStatus(`已连接 ${newest.name}（${newest.server_version}）`);
    if (newest.database) {
      updateWorkspace(newest.id, { selectedDb: newest.database });
    }
    await refreshSaved();
  }

  async function handleConnected() {
    setShowDialog(false);
    try {
      await finishConnect();
    } catch (e) {
      setStatus(String(e));
    }
  }

  async function handleReconnect(configId: string) {
    try {
      await api.reconnect(configId);
      await finishConnect();
    } catch (e) {
      setStatus(String(e));
    }
  }

  async function handleDeleteSaved(configId: string) {
    try {
      await api.deleteSavedConnection(configId);
      await refreshSaved();
    } catch (e) {
      setStatus(String(e));
    }
  }

  function handleSelectConnection(id: number) {
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
      updateWorkspace(connId, { error: String(e) });
      setStatus("生成表浏览 SQL 失败");
    }
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
              r.columns ? `返回 ${r.rows.length} 行` : `影响 ${r.affected_rows} 行`
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
      const msg = String(e);
      // 取消（#5 秒断）：连接已毒化，下次使用自动重连 —— 提示而非报错。
      // running 兜底复位（design §7）：channel 终止事件可能先于 invoke 拒绝到达，重复复位无害。
      const cancelled = msg.includes("cancelled");
      updateWorkspace(id, { error: cancelled ? null : msg, running: false });
      setStatus(cancelled ? "已取消，连接将自动重连" : "查询失败");
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
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function handleCancel() {
    if (!activeId) return;
    try {
      await api.cancelQuery(activeId);
    } catch (e) {
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function handleBegin() {
    if (!activeId) return;
    try {
      await api.begin(activeId);
      updateWorkspace(activeId, { inTransaction: true });
    } catch (e) {
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function handleCommit() {
    if (!activeId) return;
    try {
      await api.commit(activeId);
      updateWorkspace(activeId, { inTransaction: false });
      setStatus("已提交");
    } catch (e) {
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function handleRollback() {
    if (!activeId) return;
    try {
      await api.rollback(activeId);
      updateWorkspace(activeId, { inTransaction: false });
      setStatus("已回滚");
    } catch (e) {
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function handleToggleAutocommit() {
    if (!activeId) return;
    const next = !(workspaces[activeId]?.autocommit ?? true);
    try {
      await api.setAutocommit(activeId, next);
      updateWorkspace(activeId, { autocommit: next });
    } catch (e) {
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function handleEditCell(rowIndex: number, colIndex: number, newValue: string) {
    if (!activeId || !ws) return;
    const table = ws.selectedTable;
    const rs = ws.result;
    if (!table || !rs || !rs.columns) {
      setStatus("需先选择表");
      return;
    }
    const cols = rs.columns;
    const row = rs.rows[rowIndex];
    const pkCols = cols.filter((c) => c.primary_key).map((c) => c.name);
    if (pkCols.length === 0) {
      setStatus("该表无主键，无法编辑");
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
      updateWorkspace(activeId, { error: String(e) });
    }
  }

  async function confirmEdit() {
    if (!activeId || !pendingEdit) return;
    const { table, database, pk, set } = pendingEdit;
    setPendingEdit(null);
    try {
      await api.executeEdit(activeId, database, table, pk, set);
      setStatus("已更新");
      const sql = workspaces[activeId]?.query;
      if (sql) runQuery(activeId, sql);
    } catch (e) {
      updateWorkspace(activeId, { error: String(e) });
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
    const name = window.prompt("项目名称");
    if (!name) return;
    try {
      const p = await api.createProject(name);
      await refreshProjects(p.id);
    } catch (e) {
      setStatus(String(e));
    }
  }

  async function handleRenameProject() {
    if (!projectId) return;
    const current = projects.find((p) => p.id === projectId);
    const name = window.prompt("新名称", current?.name);
    if (!name) return;
    try {
      await api.renameProject(projectId, name);
      await refreshProjects();
    } catch (e) {
      setStatus(String(e));
    }
  }

  async function handleDeleteProject() {
    if (!projectId) return;
    if (!window.confirm("删除该项目？项目下连接需先删除。")) return;
    try {
      await api.deleteProject(projectId, true);
      await refreshProjects();
    } catch (e) {
      setStatus(String(e));
    }
  }

  function handleSwitchProject(id: string) {
    setProjectId(id);
    setActive(null);
  }

  async function handleDisconnect(id: number) {
    try {
      await api.disconnect(id);
      setConnections(connections.filter((c) => c.id !== id));
      closeConnection(id);
      setStatus(null);
    } catch (e) {
      setStatus(String(e));
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
    <div className="app">
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
            <span className="muted">未连接</span>
          )}
        </div>
        <div className="topbar-actions">
          <button className="btn" onClick={() => setShowDialog(true)}>
            + 新建连接
          </button>
          {activeConn && (
            <button className="btn" onClick={() => handleDisconnect(activeConn.id)}>
              断开
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
            <span className="section-title">项目</span>
            <div className="project-actions">
              <button className="btn small" onClick={handleCreateProject} title="新建项目">
                +
              </button>
              <button
                className="btn small"
                onClick={handleRenameProject}
                disabled={!projectId}
                title="重命名"
              >
                ✎
              </button>
              <button
                className="btn small"
                onClick={handleDeleteProject}
                disabled={!projectId || projects.length <= 1}
                title="删除"
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
            <span className="section-title">连接</span>
            <button className="btn small" onClick={() => setShowDialog(true)}>
              +
            </button>
          </div>
          <SchemaTree
            connections={visibleConnections}
            activeId={activeId}
            onSelectConnection={handleSelectConnection}
            onDisconnect={handleDisconnect}
            onOpenTable={handleOpenTable}
          />

          <div className="sidebar-head">
            <span className="section-title">已保存</span>
          </div>
          <div className="conn-list">
            {savedConnections
              .filter((c) => c.project_id === projectId)
              .map((c) => (
                <div key={c.id} className="list-item conn">
                  <span className="conn-dot" />
                  <span className="ellipsis" title={`${c.user}@${c.host}:${c.port}`}>
                    {c.name}
                  </span>
                  <button className="icon-btn" title="连接" onClick={() => handleReconnect(c.id)}>
                    ↻
                  </button>
                  <button
                    className="icon-btn"
                    title="删除"
                    onClick={() => handleDeleteSaved(c.id)}
                  >
                    🗑
                  </button>
                </div>
              ))}
            {savedConnections.filter((c) => c.project_id === projectId).length === 0 && (
              <div className="empty">无已保存连接</div>
            )}
          </div>
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
                  <ResultsGrid result={ws.result} onEditCell={handleEditCell} />
                ) : (
                  <div className="empty-state">选择一个表或输入 SQL 后点击「运行」</div>
                )}
              </section>
            </>
          ) : (
            <div className="empty-state">
              <h2>dby — 轻量级跨平台数据库客户端</h2>
              <p>点击「新建连接」开始，第一版支持 MySQL。</p>
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
          历史
        </button>
        {ws?.inTransaction && <span className="txn-indicator">● 事务中</span>}
        <span className="status">{status ?? "就绪"}</span>
        <span className="spacer" />
        <span>
          {drivers.length} 个驱动 · {projects.length} 个项目 · {connections.length} 个连接
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
              <h2>确认数据修改</h2>
              <button className="icon-btn" onClick={() => setPendingEdit(null)}>
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">将执行以下 SQL：</p>
              <pre className="danger-sql">{pendingEdit.sql}</pre>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingEdit(null)}>
                取消
              </button>
              <button className="btn primary" onClick={confirmEdit}>
                确认执行
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingDanger && (
        <div className="modal-backdrop" onClick={() => setPendingDanger(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>危险操作确认</h2>
              <button className="icon-btn" onClick={() => setPendingDanger(null)}>
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">以下 SQL 可能破坏数据，请确认后执行：</p>
              <pre className="danger-sql">{pendingDanger.sql}</pre>
              <ul className="danger-reasons">
                {pendingDanger.reasons.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingDanger(null)}>
                取消
              </button>
              <button
                className="btn danger"
                onClick={() => {
                  const sql = pendingDanger.sql;
                  setPendingDanger(null);
                  if (activeId) runQuery(activeId, sql, true);
                }}
              >
                确认执行
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingWarn && (
        <div className="modal-backdrop" onClick={() => setPendingWarn(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>确认执行</h2>
              <button className="icon-btn" onClick={() => setPendingWarn(null)}>
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">缺少 WHERE，可能影响大量行，请确认后执行：</p>
              <pre className="danger-sql">{pendingWarn.sql}</pre>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingWarn(null)}>
                取消
              </button>
              <button
                className="btn primary"
                onClick={() => {
                  const sql = pendingWarn.sql;
                  setPendingWarn(null);
                  if (activeId) runQuery(activeId, sql);
                }}
              >
                仍要执行
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
