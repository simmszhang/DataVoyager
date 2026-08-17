import CodeMirror from "@uiw/react-codemirror";
import { sql } from "@codemirror/lang-sql";
import { oneDark } from "@codemirror/theme-one-dark";

interface Props {
  value: string;
  running: boolean;
  onChange: (value: string) => void;
  onRun: () => void;
  onCancel: () => void;
  inTransaction: boolean;
  autocommit: boolean;
  onBegin: () => void;
  onCommit: () => void;
  onRollback: () => void;
  onToggleAutocommit: () => void;
}

export default function QueryEditor({
  value,
  running,
  onChange,
  onRun,
  onCancel,
  inTransaction,
  autocommit,
  onBegin,
  onCommit,
  onRollback,
  onToggleAutocommit,
}: Props) {
  return (
    <div className="editor-wrap">
      <div className="editor-toolbar">
        <button className="btn primary" onClick={onRun} disabled={running}>
          {running ? "执行中…" : "运行 (Ctrl+Enter)"}
        </button>
        {running && (
          <button className="btn" onClick={onCancel}>
            停止
          </button>
        )}
        <span className="toolbar-sep" />
        {inTransaction ? (
          <>
            <button className="btn" onClick={onCommit}>
              提交
            </button>
            <button className="btn" onClick={onRollback}>
              回滚
            </button>
          </>
        ) : (
          <button className="btn" onClick={onBegin}>
            开始事务
          </button>
        )}
        <label className="autocommit" title="关闭后需手动提交/回滚">
          <input type="checkbox" checked={autocommit} onChange={onToggleAutocommit} />
          autocommit
        </label>
      </div>
      <div className="editor">
        <CodeMirror
          value={value}
          height="100%"
          theme={oneDark}
          extensions={[sql()]}
          onChange={onChange}
          basicSetup={{ lineNumbers: true, autocompletion: true, highlightActiveLine: true }}
        />
      </div>
    </div>
  );
}
