import { useCallback, useEffect, useState } from "react";
import { api, ExecutionRecord, StatementHit } from "../api";

interface Props {
  projectId: string | null;
  onLoadSql: (sql: string) => void;
}

export default function HistoryPanel({ projectId, onLoadSql }: Props) {
  const [tab, setTab] = useState<"statements" | "executions">("statements");
  const [query, setQuery] = useState("");
  const [statements, setStatements] = useState<StatementHit[]>([]);
  const [executions, setExecutions] = useState<ExecutionRecord[]>([]);
  const [loading, setLoading] = useState(false);

  const refreshStatements = useCallback(async () => {
    setLoading(true);
    try {
      const q = query.trim();
      setStatements(q ? await api.searchHistory(q, projectId) : await api.listHistory(projectId));
    } catch {
      setStatements([]);
    } finally {
      setLoading(false);
    }
  }, [query, projectId]);

  const refreshExecutions = useCallback(async () => {
    setLoading(true);
    try {
      setExecutions(await api.listExecutions(projectId));
    } catch {
      setExecutions([]);
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    if (tab === "statements") refreshStatements();
    else refreshExecutions();
  }, [tab, refreshStatements, refreshExecutions]);

  async function togglePin(s: StatementHit) {
    try {
      await api.pinStatement(s.hash, !s.pinned);
      refreshStatements();
    } catch {
      /* ignore */
    }
  }

  async function handleDeleteExecution(id: string) {
    try {
      await api.deleteExecution(id);
      refreshExecutions();
    } catch {
      /* ignore */
    }
  }

  return (
    <div className="history-panel">
      <div className="history-head">
        <span className="section-title">历史</span>
        <div className="history-tabs">
          <button
            className={tab === "statements" ? "active" : ""}
            onClick={() => setTab("statements")}
          >
            语句库
          </button>
          <button
            className={tab === "executions" ? "active" : ""}
            onClick={() => setTab("executions")}
          >
            执行流水
          </button>
        </div>
        {tab === "statements" && (
          <input
            className="history-search"
            placeholder="搜索 SQL…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && refreshStatements()}
          />
        )}
        <button className="btn small" onClick={refreshStatements}>
          ⟳
        </button>
      </div>
      <div className="history-list">
        {tab === "statements"
          ? statements.map((s) => (
              <div key={s.hash} className="history-item">
                <div className="history-sql" title={s.sql} onClick={() => onLoadSql(s.sql)}>
                  {s.sql}
                </div>
                <span className="history-meta">×{s.run_count}</span>
                <button className="icon-btn" title="载入编辑器" onClick={() => onLoadSql(s.sql)}>
                  ↩
                </button>
                <button className="icon-btn" title="固定" onClick={() => togglePin(s)}>
                  {s.pinned ? "★" : "☆"}
                </button>
                <button
                  className="icon-btn"
                  title="复制"
                  onClick={() => navigator.clipboard.writeText(s.sql)}
                >
                  ⧉
                </button>
              </div>
            ))
          : executions.map((e) => (
              <div key={e.id} className="history-item">
                <div className="history-sql" title={e.sql} onClick={() => onLoadSql(e.sql)}>
                  {e.sql}
                </div>
                <span className="history-meta">
                  {e.status === "ok" ? `${e.duration_ms}ms` : "✗"}
                </span>
                <button
                  className="icon-btn"
                  title="删除"
                  onClick={() => handleDeleteExecution(e.id)}
                >
                  🗑
                </button>
              </div>
            ))}
        {(tab === "statements" ? statements : executions).length === 0 && !loading && (
          <div className="empty">暂无记录</div>
        )}
      </div>
    </div>
  );
}
