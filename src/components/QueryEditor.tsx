import CodeMirror from "@uiw/react-codemirror";
import { sql } from "@codemirror/lang-sql";
import { oneDark } from "@codemirror/theme-one-dark";
import { useTranslation } from "react-i18next";

interface Props {
  value: string;
  running: boolean;
  onChange: (value: string) => void;
  onRun: () => void;
  onCancel: () => void;
  onExport: () => void;
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
  onExport,
  inTransaction,
  autocommit,
  onBegin,
  onCommit,
  onRollback,
  onToggleAutocommit,
}: Props) {
  const { t } = useTranslation();
  return (
    <div className="editor-wrap">
      <div className="editor-toolbar">
        <button className="btn primary" onClick={onRun} disabled={running}>
          {running ? t("editor.running") : t("editor.run")}
        </button>
        {running && (
          <button className="btn" onClick={onCancel}>
            {t("editor.stop")}
          </button>
        )}
        <button className="btn" onClick={onExport} disabled={running}>
          {t("editor.export")}
        </button>
        <span className="toolbar-sep" />
        {inTransaction ? (
          <>
            <button className="btn" onClick={onCommit}>
              {t("editor.commit")}
            </button>
            <button className="btn" onClick={onRollback}>
              {t("editor.rollback")}
            </button>
          </>
        ) : (
          <button className="btn" onClick={onBegin}>
            {t("editor.beginTransaction")}
          </button>
        )}
        <label className="autocommit" title={t("editor.autocommitTitle")}>
          <input type="checkbox" checked={autocommit} onChange={onToggleAutocommit} />
          autocommit
        </label>
      </div>
      <div className="editor" onContextMenu={(e) => e.stopPropagation()}>
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
