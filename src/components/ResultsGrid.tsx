import { CellValue, StreamResult, displayCell } from "../api";

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
  if (!result.columns) {
    return (
      <div className="result-message">
        语句执行成功，影响 {result.affected_rows} 行
        {result.last_insert_id != null && `，最后插入 ID: ${result.last_insert_id}`}
      </div>
    );
  }

  const { columns, rows } = result;
  return (
    <div className="results">
      <div className="result-meta">
        返回 {rows.length} 行{result.truncated ? "（已截断，仅显示前 2000 行）" : ""}
      </div>
      <div className="grid-wrap">
        <table className="grid">
          <thead>
            <tr>
              <th className="rownum">#</th>
              {columns.map((c) => (
                <th key={c.name} title={c.type_name}>
                  <span className="th-name">{c.name}</span>
                  <span className="th-type">{c.type_name}</span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={i}>
                <td className="rownum">{i + 1}</td>
                {row.map((cell, j) => (
                  <td key={j}>
                    <Cell value={cell} />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
