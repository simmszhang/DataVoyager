import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { errToString } from "../i18n";

interface Props {
  connectionId: number;
  database: string | null;
  sql: string;
  onClose: () => void;
}

export default function ExportDialog({ connectionId, database, sql, onClose }: Props) {
  const { t } = useTranslation();
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
      const t = await api.exportResult(connectionId, database, sql, format, table || null, false);
      setText(t);
    } catch (e) {
      setError(errToString(e));
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
      setError(errToString(e));
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{t("export.dialog.title")}</h2>
          <button className="icon-btn" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          <div className="form-grid">
            <label>
              <span>{t("export.dialog.format")}</span>
              <select value={format} onChange={(e) => setFormat(e.target.value)}>
                <option value="csv">CSV</option>
                <option value="json">JSON</option>
                <option value="markdown">Markdown</option>
                <option value="insert">{t("export.dialog.formatInsert")}</option>
              </select>
            </label>
            {format === "insert" && (
              <label>
                <span>{t("export.dialog.targetTable")}</span>
                <input
                  value={table}
                  onChange={(e) => setTable(e.target.value)}
                  placeholder={t("export.dialog.tablePlaceholder")}
                />
              </label>
            )}
          </div>
          {error && <div className="form-message err">{error}</div>}
          {text && <textarea className="export-preview" readOnly value={text} rows={12} />}
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={handleExport} disabled={busy}>
            {busy ? t("export.dialog.exporting") : t("export.dialog.generate")}
          </button>
          <button className="btn primary" onClick={handleCopy} disabled={!text}>
            {copied ? t("export.dialog.copied") : t("export.dialog.copyToClipboard")}
          </button>
        </div>
      </div>
    </div>
  );
}
