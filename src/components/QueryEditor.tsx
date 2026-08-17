import CodeMirror from "@uiw/react-codemirror";
import { sql } from "@codemirror/lang-sql";
import { oneDark } from "@codemirror/theme-one-dark";

interface Props {
  value: string;
  running: boolean;
  onChange: (value: string) => void;
  onRun: () => void;
}

export default function QueryEditor({ value, running, onChange, onRun }: Props) {
  return (
    <div className="editor-wrap">
      <div className="editor-toolbar">
        <button className="btn primary" onClick={onRun} disabled={running}>
          {running ? "执行中…" : "运行 (Ctrl+Enter)"}
        </button>
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
