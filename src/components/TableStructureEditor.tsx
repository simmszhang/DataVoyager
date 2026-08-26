import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, TableColumn, AlterTableOp } from "../api";
import { errToString } from "../i18n";

interface Props {
  connId: number;
  database: string;
  table: string;
  onClose: () => void;
}

type ColumnDraft = TableColumn & {
  _action?: "add" | "modify" | "drop" | "rename";
  _oldName?: string;
};

export default function TableStructureEditor({ connId, database, table, onClose }: Props) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(true);
  const [columns, setColumns] = useState<ColumnDraft[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    loadStructure();
  }, [connId, database, table]);

  async function loadStructure() {
    setLoading(true);
    setError(null);
    try {
      const cols = await api.getTableStructure(connId, database, table);
      setColumns(cols.map((c) => ({ ...c })));
    } catch (e) {
      setError(errToString(e));
    } finally {
      setLoading(false);
    }
  }

  function handleAdd() {
    const newCol: ColumnDraft = {
      name: "",
      type_name: "VARCHAR(255)",
      nullable: true,
      default_value: null,
      comment: null,
      _action: "add",
    };
    setColumns([...columns, newCol]);
  }

  function handleDrop(index: number) {
    const col = columns[index];
    if (col._action === "add") {
      // 新增未保存，直接删除
      setColumns(columns.filter((_, i) => i !== index));
    } else {
      // 标记为删除
      setColumns(
        columns.map((c, i) => (i === index ? { ...c, _action: "drop" } : c))
      );
    }
  }

  function handleModify(index: number, field: keyof ColumnDraft, value: any) {
    setColumns(
      columns.map((c, i) => {
        if (i !== index) return c;
        const updated = { ...c, [field]: value };
        if (!c._action || c._action === "modify") {
          updated._action = "modify";
        }
        return updated;
      })
    );
  }

  async function handleSave() {
    setSaving(true);
    setError(null);

    try {
      const operations: AlterTableOp[] = [];

      for (const col of columns) {
        if (col._action === "add") {
          operations.push({
            op: "add_column",
            name: col.name,
            type_name: col.type_name,
            nullable: col.nullable,
            default_value: col.default_value,
          });
        } else if (col._action === "drop") {
          operations.push({
            op: "drop_column",
            name: col.name,
          });
        } else if (col._action === "modify") {
          operations.push({
            op: "modify_column",
            name: col.name,
            type_name: col.type_name,
            nullable: col.nullable,
            default_value: col.default_value,
          });
        } else if (col._action === "rename" && col._oldName) {
          operations.push({
            op: "rename_column",
            old_name: col._oldName,
            new_name: col.name,
          });
        }
      }

      if (operations.length === 0) {
        onClose();
        return;
      }

      await api.alterTable(connId, database, table, operations, true);
      onClose();
    } catch (e) {
      setError(errToString(e));
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return (
      <div className="modal-overlay">
        <div className="modal-content structure-editor">
          <div className="modal-header">
            <h2>{t("structure.title", { table })}</h2>
          </div>
          <div className="modal-body">
            <p>{t("structure.loading")}</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content structure-editor" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{t("structure.title", { table })}</h2>
          <button className="btn-close" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          {error && <div className="form-message err">{error}</div>}
          <table className="structure-table">
            <thead>
              <tr>
                <th>{t("structure.column.name")}</th>
                <th>{t("structure.column.type")}</th>
                <th>{t("structure.column.nullable")}</th>
                <th>{t("structure.column.default")}</th>
                <th>{t("structure.column.comment")}</th>
                <th>{t("structure.column.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {columns
                .filter((c) => c._action !== "drop")
                .map((col, index) => (
                  <tr key={index} className={col._action ? `row-${col._action}` : ""}>
                    <td>
                      <input
                        type="text"
                        value={col.name}
                        onChange={(e) => handleModify(index, "name", e.target.value)}
                        disabled={col._action === "drop"}
                      />
                    </td>
                    <td>
                      <input
                        type="text"
                        value={col.type_name}
                        onChange={(e) => handleModify(index, "type_name", e.target.value)}
                        disabled={col._action === "drop"}
                      />
                    </td>
                    <td>
                      <input
                        type="checkbox"
                        checked={col.nullable}
                        onChange={(e) => handleModify(index, "nullable", e.target.checked)}
                        disabled={col._action === "drop"}
                      />
                    </td>
                    <td>
                      <input
                        type="text"
                        value={col.default_value ?? ""}
                        onChange={(e) =>
                          handleModify(
                            index,
                            "default_value",
                            e.target.value || null
                          )
                        }
                        placeholder="NULL"
                        disabled={col._action === "drop"}
                      />
                    </td>
                    <td>
                      <input
                        type="text"
                        value={col.comment ?? ""}
                        onChange={(e) =>
                          handleModify(index, "comment", e.target.value || null)
                        }
                        disabled={col._action === "drop"}
                      />
                    </td>
                    <td>
                      <button
                        className="btn-icon-small"
                        onClick={() => handleDrop(index)}
                        disabled={col._action === "drop"}
                      >
                        🗑️
                      </button>
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
          <button className="btn" onClick={handleAdd}>
            + {t("structure.addColumn")}
          </button>
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onClose} disabled={saving}>
            {t("structure.cancel")}
          </button>
          <button className="btn primary" onClick={handleSave} disabled={saving}>
            {saving ? t("structure.saving") : t("structure.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
