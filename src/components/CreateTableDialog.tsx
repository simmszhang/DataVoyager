import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ColumnDef } from "../api";

interface Props {
  connId: number;
  database: string;
  onDone: () => void;
  onClose: () => void;
}

export default function CreateTableDialog({ connId, database, onDone, onClose }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [cols, setCols] = useState<ColumnDef[]>([
    { name: "", type_name: "VARCHAR(255)", nullable: true, primary_key: false },
  ]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function updateCol(i: number, patch: Partial<ColumnDef>) {
    setCols((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  }
  function addCol() {
    setCols((cs) => [
      ...cs,
      { name: "", type_name: "VARCHAR(255)", nullable: true, primary_key: false },
    ]);
  }
  function removeCol(i: number) {
    setCols((cs) => cs.filter((_, j) => j !== i));
  }

  async function handleCreate() {
    const tableName = name.trim();
    if (!tableName) {
      setError(t("createTable.dialog.requireName"));
      return;
    }
    const valid = cols
      .filter((c) => c.name.trim())
      .map((c) => ({ ...c, name: c.name.trim() }));
    if (valid.length === 0) {
      setError(t("createTable.dialog.requireColumn"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.createTable(connId, database, tableName, valid);
      onDone();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{t("createTable.dialog.title", { database })}</h2>
          <button className="icon-btn" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          <label className="form-field">
            <span>{t("createTable.dialog.tableName")}</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("createTable.dialog.tableNamePlaceholder")}
            />
          </label>
          <div className="col-defs">
            <div className="col-def-head">
              <span>{t("createTable.dialog.columnName")}</span>
              <span>{t("createTable.dialog.type")}</span>
              <span title={t("createTable.dialog.notNull")}>{t("createTable.dialog.notNull")}</span>
              <span title={t("createTable.dialog.primaryKey")}>
                {t("createTable.dialog.primaryKey")}
              </span>
              <span />
            </div>
            {cols.map((c, i) => (
              <div key={i} className="col-def-row">
                <input
                  value={c.name}
                  onChange={(e) => updateCol(i, { name: e.target.value })}
                  placeholder={t("createTable.dialog.columnNamePlaceholder")}
                />
                <input
                  value={c.type_name}
                  onChange={(e) => updateCol(i, { type_name: e.target.value })}
                  placeholder="VARCHAR(255)"
                />
                <input
                  type="checkbox"
                  checked={!c.nullable}
                  onChange={(e) => updateCol(i, { nullable: !e.target.checked })}
                />
                <input
                  type="checkbox"
                  checked={c.primary_key}
                  onChange={(e) => updateCol(i, { primary_key: e.target.checked })}
                />
                <button className="icon-btn" onClick={() => removeCol(i)}>
                  ✕
                </button>
              </div>
            ))}
            <button className="btn small" onClick={addCol}>
              {t("createTable.dialog.addColumn")}
            </button>
          </div>
          {error && <div className="form-message err">{error}</div>}
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onClose}>
            {t("createTable.dialog.cancel")}
          </button>
          <button className="btn primary" onClick={handleCreate} disabled={busy}>
            {busy ? t("createTable.dialog.creating") : t("createTable.dialog.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
