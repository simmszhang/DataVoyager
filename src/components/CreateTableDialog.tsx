import { useState } from "react";
import { api, ColumnDef } from "../api";

interface Props {
  connId: number;
  database: string;
  onDone: () => void;
  onClose: () => void;
}

export default function CreateTableDialog({ connId, database, onDone, onClose }: Props) {
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
    const t = name.trim();
    if (!t) {
      setError("请输入表名");
      return;
    }
    const valid = cols
      .filter((c) => c.name.trim())
      .map((c) => ({ ...c, name: c.name.trim() }));
    if (valid.length === 0) {
      setError("至少需要一个列");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.createTable(connId, database, t, valid);
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
          <h2>新建表（{database}）</h2>
          <button className="icon-btn" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          <label className="form-field">
            <span>表名</span>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="表名" />
          </label>
          <div className="col-defs">
            <div className="col-def-head">
              <span>列名</span>
              <span>类型</span>
              <span title="非空">非空</span>
              <span title="主键">主键</span>
              <span />
            </div>
            {cols.map((c, i) => (
              <div key={i} className="col-def-row">
                <input
                  value={c.name}
                  onChange={(e) => updateCol(i, { name: e.target.value })}
                  placeholder="列名"
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
              + 添加列
            </button>
          </div>
          {error && <div className="form-message err">{error}</div>}
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" onClick={handleCreate} disabled={busy}>
            {busy ? "创建中…" : "创建"}
          </button>
        </div>
      </div>
    </div>
  );
}
