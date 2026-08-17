import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  ColumnInfo,
  ConnectionSummary,
  DriverInfo,
  Project,
  QueryOutput,
  TableInfo,
} from "./api";
import ConnectionDialog from "./components/ConnectionDialog";
import SchemaPanel from "./components/SchemaPanel";
import QueryEditor from "./components/QueryEditor";
import ResultsGrid from "./components/ResultsGrid";
import "./App.css";

export default function App() {
  const [drivers, setDrivers] = useState<DriverInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [connections, setConnections] = useState<ConnectionSummary[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [showDialog, setShowDialog] = useState(false);

  const [databases, setDatabases] = useState<string[]>([]);
  const [selectedDb, setSelectedDb] = useState("");
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [selectedTable, setSelectedTable] = useState<string | null>(null);
  const [columns, setColumns] = useState<ColumnInfo[]>([]);
  const [schemaLoading, setSchemaLoading] = useState(false);

  const [query, setQuery] = useState("SELECT 1");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<QueryOutput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const activeConn = connections.find((c) => c.id === activeId) ?? null;

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

  async function handleConnected() {
    setShowDialog(false);
    try {
      const list = await api.listConnections();
      setConnections(list);
      const newest = list[list.length - 1];
      setActiveId(newest.id);
      setStatus(`已连接 ${newest.name}（MySQL ${newest.server_version}）`);
      setSelectedTable(null);
      setColumns([]);
      setResult(null);
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
    setResult(null);
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

  async function handleRun() {
    if (!activeId) return;
    setRunning(true);
    setError(null);
    try {
      const res = await api.executeQuery(activeId, selectedDb || null, query);
      setResult(res);
      setStatus(
        res.result_sets.length === 0
          ? `影响 ${res.affected_rows} 行`
          : `返回 ${res.result_sets[0]?.rows.length ?? 0} 行`,
      );
    } catch (e) {
      setError(String(e));
      setStatus("查询失败");
    } finally {
      setRunning(false);
    }
  }

  // Keep the latest run handler available to the global shortcut without stale closures.
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
        setResult(null);
        setStatus(null);
        setError(null);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">dby</div>
        <div className="conn-info">
          {activeConn ? (
            <>
              <span className="conn-name">{activeConn.name}</span>
              <span className="conn-ver">MySQL {activeConn.server_version}</span>
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
            <span className="section-title">连接</span>
            <button className="btn small" onClick={() => setShowDialog(true)}>
              +
            </button>
          </div>
          <div className="conn-list">
            {connections.map((c) => (
              <div
                key={c.id}
                className={`list-item conn ${activeId === c.id ? "active" : ""}`}
                onClick={() => handleSelectConnection(c.id)}
              >
                <span className="conn-dot" />
                <span className="ellipsis">{c.name}</span>
              </div>
            ))}
            {connections.length === 0 && <div className="empty">暂无连接</div>}
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
        </main>
      </div>

      <footer className="statusbar">
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
    </div>
  );
}
