import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  CellValue,
  ColumnInfo,
  ColumnTypeBase,
  StreamResult,
  displayCell,
} from "../api";

const ROW_HEIGHT = 30;
const COL_WIDTH = 200;
const ROWNUM_WIDTH = 56;

interface Props {
  result: StreamResult;
  onEditCell?: (rowIndex: number, colIndex: number, newValue: string) => void;
}

function Cell({ value }: { value: CellValue }) {
  const { t } = useTranslation();
  switch (value.t) {
    case "null":
      return <span className="cell-null">{t("grid.null")}</span>;
    case "i64":
    case "u64":
    case "f64":
      return <span className="cell-number">{displayCell(value)}</span>;
    case "bool":
      return <span className="cell-bool">{displayCell(value)}</span>;
    default:
      return <span className="cell-text">{displayCell(value)}</span>;
  }
}

const NUMERIC_BASES: ReadonlySet<ColumnTypeBase> = new Set([
  "i8",
  "i16",
  "i32",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "f32",
  "f64",
  "decimal",
]);

type EditorKind = "number" | "bool" | "json" | "text";

/// 按列结构化类型（ColumnInfo.column_type.base）选编辑控件（#1）；
/// 列类型缺失时退化为普通文本输入。
function editorKind(col: ColumnInfo): EditorKind {
  const base = col.column_type?.base ?? "unknown";
  if (NUMERIC_BASES.has(base)) return "number";
  if (base === "bool") return "bool";
  if (base === "json") return "json";
  return "text";
}

interface CellEditorProps {
  kind: EditorKind;
  text: string;
  onChange: (text: string) => void;
  onCommit: () => void;
  onCancel: () => void;
}

/// 编辑态控件：bool → checkbox、json → textarea、numeric → number input、其余 → text input。
/// 所有控件都以「原始输入串」上抛，由后端 parse_value 按列类型解析（#11/#69）。
function CellEditor({ kind, text, onChange, onCommit, onCancel }: CellEditorProps) {
  const { t } = useTranslation();
  if (kind === "bool") {
    return (
      <input
        type="checkbox"
        className="cell-checkbox"
        autoFocus
        checked={text === "true"}
        onChange={(e) => onChange(e.target.checked ? "true" : "false")}
        onBlur={onCommit}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
        }}
      />
    );
  }
  if (kind === "json") {
    return (
      <textarea
        className="cell-input"
        autoFocus
        rows={4}
        value={text}
        placeholder={t("grid.null")}
        onChange={(e) => onChange(e.target.value)}
        onBlur={onCommit}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) onCommit();
          else if (e.key === "Escape") onCancel();
        }}
      />
    );
  }
  return (
    <input
      className="cell-input"
      type={kind === "number" ? "number" : "text"}
      step="any"
      autoFocus
      value={text}
      placeholder={t("grid.null")}
      onChange={(e) => onChange(e.target.value)}
      onBlur={onCommit}
      onKeyDown={(e) => {
        if (e.key === "Enter") onCommit();
        else if (e.key === "Escape") onCancel();
      }}
    />
  );
}

export default function ResultsGrid({ result, onEditCell }: Props) {
  const { t } = useTranslation();
  const parentRef = useRef<HTMLDivElement>(null);
  const [editing, setEditing] = useState<{ row: number; col: number; text: string } | null>(null);
  const rows = result.columns ? result.rows : [];
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  function commitEdit() {
    if (editing && onEditCell) onEditCell(editing.row, editing.col, editing.text);
    setEditing(null);
  }

  if (!result.columns) {
    return (
      <div className="result-message">
        {t("grid.executedAffected", { count: result.affected_rows })}
        {result.last_insert_id != null &&
          t("grid.lastInsertId", { id: result.last_insert_id })}
        {result.truncated && t("grid.truncatedSuffix")}
      </div>
    );
  }

  const { columns } = result;
  const colTemplate = `${ROWNUM_WIDTH}px ${columns
    .map(() => `${COL_WIDTH}px`)
    .join(" ")}`;

  return (
    <div className="results">
      <div className="result-meta">
        {t("grid.rowsReturned", { count: rows.length })}
        {result.truncated && (
          <span
            title={t("grid.truncatedTitle")}
            style={{ color: "#e0a63c", fontWeight: 600 }}
          >
            {t("grid.truncated")}
          </span>
        )}
        {onEditCell && t("grid.doubleClickEdit")}
      </div>
      <div className="grid-wrap" ref={parentRef}>
        <div style={{ minWidth: "100%", width: "max-content" }}>
          <div
            className="grid-header"
            style={{ display: "grid", gridTemplateColumns: colTemplate }}
          >
            <div className="grid-cell rownum">#</div>
            {columns.map((c) => (
              <div className="grid-cell" key={c.name} title={c.type_name}>
                <span className="th-name">{c.name}</span>
                <span className="th-type">{c.type_name}</span>
              </div>
            ))}
          </div>
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: "relative",
              width: "100%",
            }}
          >
            {virtualizer.getVirtualItems().map((vi) => {
              const row = rows[vi.index];
              return (
                <div
                  key={vi.key}
                  className="grid-row"
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: vi.size,
                    transform: `translateY(${vi.start}px)`,
                    display: "grid",
                    gridTemplateColumns: colTemplate,
                  }}
                >
                  <div className="grid-cell rownum">{vi.index + 1}</div>
                  {row.map((cell, j) => {
                    const isEditing = editing?.row === vi.index && editing?.col === j;
                    return (
                      <div
                        className="grid-cell"
                        key={j}
                        onDoubleClick={() =>
                          onEditCell &&
                          setEditing({
                            row: vi.index,
                            col: j,
                            text: cell.t === "null" ? "" : displayCell(cell),
                          })
                        }
                      >
                        {isEditing ? (
                          <CellEditor
                            kind={editorKind(columns[j])}
                            text={editing!.text}
                            onChange={(text) => setEditing({ ...editing!, text })}
                            onCommit={commitEdit}
                            onCancel={() => setEditing(null)}
                          />
                        ) : (
                          <Cell value={cell} />
                        )}
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
