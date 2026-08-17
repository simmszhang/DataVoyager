import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { CellValue, StreamResult, displayCell } from "../api";

const ROW_HEIGHT = 30;
const COL_WIDTH = 200;
const ROWNUM_WIDTH = 56;

interface Props {
  result: StreamResult;
}

function Cell({ value }: { value: CellValue }) {
  switch (value.t) {
    case "null":
      return <span className="cell-null">NULL</span>;
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

export default function ResultsGrid({ result }: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const rows = result.columns ? result.rows : [];
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  if (!result.columns) {
    return (
      <div className="result-message">
        语句执行成功，影响 {result.affected_rows} 行
        {result.last_insert_id != null && `，最后插入 ID: ${result.last_insert_id}`}
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
        返回 {rows.length} 行{result.truncated ? "（已截断，仅显示前 2000 行）" : ""}
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
                  {row.map((cell, j) => (
                    <div className="grid-cell" key={j}>
                      <Cell value={cell} />
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
