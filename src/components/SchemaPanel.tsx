import { useTranslation } from "react-i18next";
import { ColumnInfo, TableInfo } from "../api";

interface Props {
  databases: string[];
  tables: TableInfo[];
  columns: ColumnInfo[];
  selectedDb: string;
  selectedTable: string | null;
  loading: boolean;
  onSelectDb: (db: string) => void;
  onSelectTable: (table: string) => void;
  onRefresh: () => void;
}

export default function SchemaPanel({
  databases,
  tables,
  columns,
  selectedDb,
  selectedTable,
  loading,
  onSelectDb,
  onSelectTable,
  onRefresh,
}: Props) {
  const { t } = useTranslation();
  return (
    <div className="schema-panel">
      <div className="panel-section">
        <div className="section-row">
          <span className="section-title">{t("schema.databases")}</span>
          <button
            className="icon-btn"
            onClick={onRefresh}
            disabled={loading}
            aria-label={t("schema.refresh")}
          >
            ⟳
          </button>
        </div>
        <select
          value={selectedDb}
          onChange={(e) => onSelectDb(e.target.value)}
          disabled={loading}
        >
          <option value="">{t("schema.selectDatabase")}</option>
          {databases.map((db) => (
            <option key={db} value={db}>
              {db}
            </option>
          ))}
        </select>
      </div>

      <div className="panel-section grow">
        <div className="section-row">
          <span className="section-title">{t("schema.tables")}</span>
          <span className="count">{tables.length}</span>
        </div>
        <div className="list">
          {tables.map((t) => (
            <div
              key={t.name}
              className={`list-item ${selectedTable === t.name ? "active" : ""}`}
              onClick={() => onSelectTable(t.name)}
              title={t.kind ?? ""}
            >
              <span className="table-icon">▤</span>
              <span className="ellipsis">{t.name}</span>
            </div>
          ))}
          {!loading && selectedDb && tables.length === 0 && (
            <div className="empty">{t("schema.noTables")}</div>
          )}
        </div>
      </div>

      {selectedTable && (
        <div className="panel-section columns">
          <div className="section-row">
            <span className="section-title">{t("schema.columns")}</span>
            <span className="count">{columns.length}</span>
          </div>
          <div className="list">
            {columns.map((c) => (
              <div key={c.name} className="list-item column">
                <span className="col-name ellipsis" title={c.name}>
                  {c.name}
                </span>
                <span className="col-type" title={c.type_name}>
                  {c.type_name}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
