import { useState } from "react";
import { api } from "../api";

interface Props {
  connectionId: number;
  database: string | null;
  sql: string;
  onClose: () => void;
}

export default function ExportDialog({ connectionId, database, sql, onClose }: Props) {
  const [format, setFormat] = useState("csv");
  const [table, setTable] = useState("");
  const [busy, setBusy] = useState(false);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  async function handleExport() {
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      const t = await api.exportResult(connectionId, database, sql, format, table || null);
      setText(t);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleCopy() {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>导出结果</h2>
          <button className="icon-btn" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          <div className="form-grid">
            <label>
              <span>格式</span>
              <select value={format} onChange={(e) => setFormat(e.target.value)}>
                <option value="csv">CSV</option>
                <option value="json">JSON</option>
                <option value="markdown">Markdown</option>
                <option value="insert">INSERT 语句</option>
              </select>
            </label>
            {format === "insert" && (
              <label>
                <span>目标表名</span>
                <input
                  value={table}
                  onChange={(e) => setTable(e.target.value)}
                  placeholder="表名"
                />
              </label>
            )}
          </div>
          {error && <div className="form-message err">{error}</div>}
          {text && <textarea className="export-preview" readOnly value={text} rows={12} />}
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={handleExport} disabled={busy}>
            {busy ? "导出中…" : "生成"}
          </button>
          <button className="btn primary" onClick={handleCopy} disabled={!text}>
            {copied ? "已复制 ✓" : "复制到剪贴板"}
          </button>
        </div>
      </div>
    </div>
  );
}
