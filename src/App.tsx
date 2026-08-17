import { useCallback, useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import {
  api,
  ColumnInfo,
  ConnectionSummary,
  DriverInfo,
  Project,
  StreamEvent,
  StreamResult,
  TableInfo,
} from "./api";
import ConnectionDialog from "./components/ConnectionDialog";
import SchemaPanel from "./components/SchemaPanel";
import QueryEditor from "./components/QueryEditor";
import ResultsGrid from "./components/ResultsGrid";
import ExportDialog from "./components/ExportDialog";
import HistoryPanel from "./components/HistoryPanel";
import "./App.css";

export default function App() {
  const [drivers, setDrivers] = useState<DriverInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [connections, setConnections] = useState<ConnectionSummary[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [showDialog, setShowDialog] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [showHistory, setShowHistory] = useState(false);

  const [databases, setDatabases] = useState<string[]>([]);
  const [selectedDb, setSelectedDb] = useState("");
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [selectedTable, setSelectedTable] = useState<string | null>(null);
  const [columns, setColumns] = useState<ColumnInfo[]>([]);
  const [schemaLoading, setSchemaLoading] = useState(false);

  const [query, setQuery] = useState("SELECT 1");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [pendingDanger, setPendingDanger] = useState<{
    sql: string;
    reasons: string[];
  } | null>(null);
  const [inTransaction, setInTransaction] = useState(false);
  const [autocommit, setAutocommit] = useState(true);

  // 流式结果：ref 持有可变缓冲，tick 触发渲染（O(1) 追加）。
  const resultRef = useRef<StreamResult | null>(null);
  const [, setTick] = useState(0);

  const activeConn = connections.find((c) => c.id === activeId) ?? null;
  const visibleConnections = connections.filter((c) => c.project_id === projectId);

  useEffect(() => {
    api
      .listDrivers()
      .then(setDrivers)
      .catch((e) => setError(String(e)));
    api
      .listProjects()
      .then((ps) => {
        setProjects(ps);
        if (ps.length > 0) setProjectId(ps[0].id);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const loadDatabases = useCallback(async (id: number) => {
    try {
      const dbs = await api.listDatabases(id);
      setDatabases(dbs);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  function resetResult() {
    resultRef.current = null;
    setTick((t) => t + 1);
  }

  async function handleConnected() {
    setShowDialog(false);
    try {
      const list = await api.listConnections();
      setConnections(list);
      const newest = list[list.length - 1];
      setActiveId(newest.id);
      setStatus(`已连接 ${newest.name}（${newest.server_version}）`);
      setSelectedTable(null);
      setColumns([]);
      resetResult();
      setError(null);
      await loadDatabases(newest.id);
      const db = newest.database;
      setSelectedDb(db);
      if (db) {
        setTables(await api.listTables(newest.id, db));
      } else {
        setTables([]);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleSelectConnection(id: number) {
    setActiveId(id);
    resetResult();
    resetTxn();
    setError(null);
    setSelectedTable(null);
    setColumns([]);
    const conn = connections.find((c) => c.id === id);
    await loadDatabases(id);
    const db = conn?.database ?? "";
    setSelectedDb(db);
    if (db) {
      try {
        setTables(await api.listTables(id, db));
      } catch (e) {
        setError(String(e));
      }
    } else {
      setTables([]);
    }
  }

  async function handleSelectDb(db: string) {
    setSelectedDb(db);
    setSelectedTable(null);
    setColumns([]);
    if (!activeId) return;
    if (!db) {
      setTables([]);
      return;
    }
    setSchemaLoading(true);
    try {
      setTables(await api.listTables(activeId, db));
    } catch (e) {
      setError(String(e));
    } finally {
      setSchemaLoading(false);
    }
  }

  async function handleSelectTable(table: string) {
    setSelectedTable(table);
    if (!activeId || !selectedDb) return;
    try {
      const cols = await api.listColumns(activeId, selectedDb, table);
      setColumns(cols);
      setQuery(`SELECT * FROM \`${table}\` LIMIT 100;`);
    } catch (e) {
      setError(String(e));
    }
  }

  async function runQuery(sql: string) {
    if (!activeId) return;
    setRunning(true);
    setError(null);
    resultRef.current = {
      columns: null,
      rows: [],
      affected_rows: 0,
      last_insert_id: null,
      truncated: false,
    };
    setTick((t) => t + 1);

    const channel = new Channel<StreamEvent>();
    channel.onmessage = (ev) => {
      const r = resultRef.current;
      if (!r) return;
      switch (ev.event) {
        case "columns":
          r.columns = ev.data;
          break;
        case "rows":
          r.rows.push(...ev.data);
          break;
        case "affected":
          r.affected_rows = ev.data.affected_rows;
          r.last_insert_id = ev.data.last_insert_id;
          break;
        case "info":
          break;
      }
      setTick((t) => t + 1);
    };

    try {
      await api.executeQueryStream(channel, activeId, selectedDb || null, sql);
      const r = resultRef.current;
      setStatus(
        r && r.columns
          ? `返回 ${r.rows.length} 行`
          : `影响 ${r?.affected_rows ?? 0} 行`,
      );
    } catch (e) {
      setError(String(e));
      setStatus("查询失败");
    } finally {
      setRunning(false);
    }
  }

  async function handleRun() {
    if (!activeId) return;
    try {
      const danger = await api.analyzeDanger(query);
      if (danger.level === "dangerous") {
        setPendingDanger({ sql: query, reasons: danger.reasons });
        return;
      }
      await runQuery(query);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleCancel() {
    if (!activeId) return;
    try {
      await api.cancelQuery(activeId);
    } catch (e) {
      setError(String(e));
    }
  }

  function resetTxn() {
    setInTransaction(false);
    setAutocommit(true);
  }

  async function handleBegin() {
    if (!activeId) return;
    try {
      await api.begin(activeId);
      setInTransaction(true);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleCommit() {
    if (!activeId) return;
    try {
      await api.commit(activeId);
      setInTransaction(false);
      setStatus("已提交");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRollback() {
    if (!activeId) return;
    try {
      await api.rollback(activeId);
      setInTransaction(false);
      setStatus("已回滚");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleToggleAutocommit() {
    if (!activeId) return;
    const next = !autocommit;
    try {
      await api.setAutocommit(activeId, next);
      setAutocommit(next);
    } catch (e) {
      setError(String(e));
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
      setError(String(e));
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
      setError(String(e));
    }
  }

  async function handleDeleteProject() {
    if (!projectId) return;
    if (!window.confirm("删除该项目？项目下连接需先删除。")) return;
    try {
      await api.deleteProject(projectId);
      await refreshProjects();
    } catch (e) {
      setError(String(e));
    }
  }

  function handleSwitchProject(id: string) {
    setProjectId(id);
    setActiveId(null);
    resetResult();
    resetTxn();
    setError(null);
    setDatabases([]);
    setSelectedDb("");
    setTables([]);
    setSelectedTable(null);
    setColumns([]);
  }

  // 快捷键：Ctrl/Cmd+Enter 运行（通过 ref 避免闭包过期）。
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

  async function handleDisconnect(id: number) {
    try {
      await api.disconnect(id);
      setConnections((list) => list.filter((c) => c.id !== id));
      if (activeId === id) {
        setActiveId(null);
        setDatabases([]);
        setSelectedDb("");
        setTables([]);
        setSelectedTable(null);
        setColumns([]);
        resetResult();
        resetTxn();
        setStatus(null);
        setError(null);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  const result = resultRef.current;

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
          <div className="conn-list">
            {visibleConnections.map((c) => (
              <div
                key={c.id}
                className={`list-item conn ${activeId === c.id ? "active" : ""}`}
                onClick={() => handleSelectConnection(c.id)}
                title={c.server_version}
              >
                <span className="conn-dot" />
                <span className="ellipsis">{c.name}</span>
                <span className="conn-driver">{c.driver_id}</span>
              </div>
            ))}
            {visibleConnections.length === 0 && <div className="empty">暂无连接</div>}
          </div>

          {activeConn && (
            <SchemaPanel
              databases={databases}
              tables={tables}
              columns={columns}
              selectedDb={selectedDb}
              selectedTable={selectedTable}
              loading={schemaLoading}
              onSelectDb={handleSelectDb}
              onSelectTable={handleSelectTable}
              onRefresh={() => loadDatabases(activeConn.id)}
            />
          )}
        </aside>

        <main className="main">
          {activeConn ? (
            <>
              <QueryEditor
                value={query}
                running={running}
                onChange={setQuery}
                onRun={handleRun}
                onCancel={handleCancel}
                onExport={() => setShowExport(true)}
                inTransaction={inTransaction}
                autocommit={autocommit}
                onBegin={handleBegin}
                onCommit={handleCommit}
                onRollback={handleRollback}
                onToggleAutocommit={handleToggleAutocommit}
              />
              <section className="results-panel">
                {error ? (
                  <div className="error-box">{error}</div>
                ) : result ? (
                  <ResultsGrid result={result} />
                ) : (
                  <div className="empty-state">
                    选择一个表或输入 SQL 后点击「运行」
                  </div>
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
            <HistoryPanel projectId={projectId} onLoadSql={(sql) => setQuery(sql)} />
          )}
        </main>
      </div>

      <footer className="statusbar">
        <button className="btn small" onClick={() => setShowHistory((v) => !v)}>
          历史
        </button>
        {inTransaction && <span className="txn-indicator">● 事务中</span>}
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
          database={selectedDb || null}
          sql={query}
          onClose={() => setShowExport(false)}
        />
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
                  runQuery(sql);
                }}
              >
                确认执行
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
