import { ReactElement, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ColumnInfo, ConnectionSummary, TableInfo, ViewInfo, ProcedureInfo, TriggerInfo } from "../api";
import { errToString } from "../i18n";
import CreateTableDialog from "./CreateTableDialog";

type MenuNode =
  | { kind: "connection"; connId: number }
  | { kind: "database"; connId: number; name: string }
  | { kind: "tables-group"; connId: number; database: string }
  | { kind: "views-group"; connId: number; database: string }
  | { kind: "functions-group"; connId: number; database: string }
  | { kind: "procedures-group"; connId: number; database: string }
  | { kind: "table"; connId: number; database: string; name: string }
  | { kind: "view"; connId: number; database: string; name: string }
  | { kind: "function"; connId: number; database: string; name: string }
  | { kind: "procedure"; connId: number; database: string; name: string }
  | { kind: "trigger"; connId: number; database: string; name: string };

interface Props {
  connections: ConnectionSummary[];
  activeId: number | null;
  onSelectConnection: (id: number) => void;
  onDisconnect: (id: number) => void;
  onReconnect: (configId: string) => void; // R11: 重连已保存的连接
  onDeleteConnection: (configId: string) => void; // #73: 删除保存的连接
  onOpenTable: (connId: number, database: string, table: string) => void;
  onShowDDL: (connId: number, database: string, table: string) => void;
  onShowObjectDDL: (connId: number, database: string, objectName: string, objectType: string) => void; // 查看视图/函数/存储过程/触发器的 DDL
  onEditStructure: (connId: number, database: string, table: string) => void;
  onInsertTemplate: (connId: number, template: string) => void; // #75: 插入 DDL 模板
}

/// 结构化节点 key：JSON 编码，避免 `:` 拼接/切分在库表名含分隔符时失效（defect #3）。
type NodeKey =
  | { kind: "conn"; connId: number }
  | { kind: "db"; connId: number; db: string }
  | { kind: "tables-group"; connId: number; db: string }
  | { kind: "views-group"; connId: number; db: string }
  | { kind: "functions-group"; connId: number; db: string }
  | { kind: "procedures-group"; connId: number; db: string }
  | { kind: "triggers-group"; connId: number; db: string }
  | { kind: "table"; connId: number; db: string; table: string }
  | { kind: "view"; connId: number; db: string; view: string }
  | { kind: "function"; connId: number; db: string; func: string }
  | { kind: "procedure"; connId: number; db: string; proc: string }
  | { kind: "trigger"; connId: number; db: string; trigger: string }
  | { kind: "column"; connId: number; db: string; table: string; column: string };

const keyOf = (k: NodeKey): string => JSON.stringify(k);
const parseKey = (key: string): NodeKey => JSON.parse(key);

const connKey = (id: number) => keyOf({ kind: "conn", connId: id });
const dbKey = (id: number, db: string) => keyOf({ kind: "db", connId: id, db });
const tablesGroupKey = (id: number, db: string) => keyOf({ kind: "tables-group", connId: id, db });
const viewsGroupKey = (id: number, db: string) => keyOf({ kind: "views-group", connId: id, db });
const functionsGroupKey = (id: number, db: string) => keyOf({ kind: "functions-group", connId: id, db });
const proceduresGroupKey = (id: number, db: string) => keyOf({ kind: "procedures-group", connId: id, db });
const triggersGroupKey = (id: number, db: string) => keyOf({ kind: "triggers-group", connId: id, db });
const tblKey = (id: number, db: string, t: string) =>
  keyOf({ kind: "table", connId: id, db, table: t });
const viewKey = (id: number, db: string, v: string) =>
  keyOf({ kind: "view", connId: id, db, view: v });
const funcKey = (id: number, db: string, f: string) =>
  keyOf({ kind: "function", connId: id, db, func: f });
const procKey = (id: number, db: string, p: string) =>
  keyOf({ kind: "procedure", connId: id, db, proc: p });
const triggerKey = (id: number, db: string, t: string) =>
  keyOf({ kind: "trigger", connId: id, db, trigger: t });

export default function SchemaTree({
  connections,
  activeId,
  onSelectConnection,
  onDisconnect,
  onReconnect,
  onDeleteConnection,
  onOpenTable,
  onShowDDL,
  onShowObjectDDL,
  onEditStructure,
  onInsertTemplate,
}: Props) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [dbs, setDbs] = useState<Record<string, string[]>>({});
  const [tables, setTables] = useState<Record<string, TableInfo[]>>({});
  const [views, setViews] = useState<Record<string, ViewInfo[]>>({});
  const [functions, setFunctions] = useState<Record<string, ProcedureInfo[]>>({});
  const [procedures, setProcedures] = useState<Record<string, ProcedureInfo[]>>({});
  const [triggers, setTriggers] = useState<Record<string, TriggerInfo[]>>({});
  const [columns, setColumns] = useState<Record<string, ColumnInfo[]>>({});
  const [menu, setMenu] = useState<{ x: number; y: number; node: MenuNode } | null>(null);
  const [createTable, setCreateTable] = useState<{ connId: number; database: string } | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  async function loadChildren(key: string) {
    const node = parseKey(key);
    try {
      if (node.kind === "conn") {
        const list = await api.listDatabases(node.connId);
        setDbs((p) => ({ ...p, [key]: list }));
      } else if (node.kind === "db") {
        // Database node doesn't load anything automatically
        // Children (groups) are always shown
      } else if (node.kind === "tables-group") {
        const list = await api.listTables(node.connId, node.db);
        setTables((p) => ({ ...p, [key]: list }));
      } else if (node.kind === "views-group") {
        const list = await api.listViews(node.connId, node.db);
        setViews((p) => ({ ...p, [key]: list }));
      } else if (node.kind === "functions-group") {
        const list = await api.listFunctions(node.connId, node.db);
        setFunctions((p) => ({ ...p, [key]: list }));
      } else if (node.kind === "procedures-group") {
        const list = await api.listProcedures(node.connId, node.db);
        setProcedures((p) => ({ ...p, [key]: list }));
      } else if (node.kind === "triggers-group") {
        const list = await api.listTriggers(node.connId, node.db);
        setTriggers((p) => ({ ...p, [key]: list }));
      } else if (node.kind === "table") {
        const list = await api.listColumns(node.connId, node.db, node.table);
        setColumns((p) => ({ ...p, [key]: list }));
      }
    } catch (e) {
      setStatus(errToString(e));
    }
  }

  async function toggle(key: string) {
    if (expanded.has(key)) {
      setExpanded((s) => new Set([...s].filter((k) => k !== key)));
      return;
    }
    await loadChildren(key);
    setExpanded((s) => new Set([...s, key]));
  }

  function openMenu(e: React.MouseEvent, node: MenuNode) {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, node });
  }

  function handleRenameTable(n: { connId: number; database: string; name: string }) {
    const newName = window.prompt(t("tree.renamePrompt"), n.name);
    if (!newName || newName === n.name) return;
    api
      .renameTable(n.connId, n.database, n.name, newName, true)
      .then(() => loadChildren(dbKey(n.connId, n.database)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleDropDatabase(n: { connId: number; name: string }) {
    if (!window.confirm(t("tree.dropDatabaseConfirm", { name: n.name }))) return;
    api
      .dropDatabase(n.connId, n.name, true)
      .then(() => loadChildren(connKey(n.connId)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleDropTable(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(t("tree.dropTableConfirm", { name: n.name }))) return;
    api
      .dropTable(n.connId, n.database, n.name, true)
      .then(() => loadChildren(tablesGroupKey(n.connId, n.database)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleDropView(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(t("tree.dropViewConfirm", { name: n.name }))) return;
    api
      .dropView(n.connId, n.database, n.name, true)
      .then(() => loadChildren(viewsGroupKey(n.connId, n.database)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleDropFunction(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(t("tree.dropFunctionConfirm", { name: n.name }))) return;
    api
      .dropRoutine(n.connId, n.database, n.name, "FUNCTION", true)
      .then(() => loadChildren(functionsGroupKey(n.connId, n.database)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleDropProcedure(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(t("tree.dropProcedureConfirm", { name: n.name }))) return;
    api
      .dropRoutine(n.connId, n.database, n.name, "PROCEDURE", true)
      .then(() => loadChildren(proceduresGroupKey(n.connId, n.database)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleDropTrigger(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(t("tree.dropTriggerConfirm", { name: n.name }))) return;
    api
      .dropTrigger(n.connId, n.database, n.name, true)
      .then(() => loadChildren(triggersGroupKey(n.connId, n.database)))
      .catch((e) => setStatus(errToString(e)));
  }

  function handleTruncateTable(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(t("tree.truncateConfirm", { name: n.name }))) return;
    api
      .truncateTable(n.connId, n.database, n.name, true)
      .then(() => setStatus(t("tree.truncateSuccess")))
      .catch((e) => setStatus(errToString(e)));
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text).catch(() => {});
  }

  const nodes: ReactElement[] = [];
  for (const c of connections) {
    const ck = connKey(c.id);
    const cExpanded = expanded.has(ck);
    nodes.push(
      <div
        key={ck}
        className={`tree-node ${activeId === c.id ? "active" : ""}`}
        style={{ paddingLeft: 4 }}
        onClick={() => {
          onSelectConnection(c.id);
          toggle(ck);
        }}
        onContextMenu={(e) => openMenu(e, { kind: "connection", connId: c.id })}
      >
        <span className="tree-caret">{cExpanded ? "▾" : "▸"}</span>
        <span className="tree-icon">🖥</span>
        <span className="tree-label ellipsis">{c.name}</span>
        <span className="tree-tag">{c.driver_id}</span>
      </div>,
    );

    if (cExpanded) {
      for (const db of dbs[ck] ?? []) {
        const dk = dbKey(c.id, db);
        const dExpanded = expanded.has(dk);
        nodes.push(
          <div
            key={dk}
            className="tree-node"
            style={{ paddingLeft: 22 }}
            onClick={() => toggle(dk)}
            onContextMenu={(e) => openMenu(e, { kind: "database", connId: c.id, name: db })}
          >
            <span className="tree-caret">{dExpanded ? "▾" : "▸"}</span>
            <span className="tree-icon">🗄</span>
            <span className="tree-label ellipsis">{db}</span>
          </div>,
        );
        if (dExpanded) {
          // Tables group
          const tablesGk = tablesGroupKey(c.id, db);
          const tablesGExpanded = expanded.has(tablesGk);
          nodes.push(
            <div
              key={tablesGk}
              className="tree-node"
              style={{ paddingLeft: 40 }}
              onClick={() => toggle(tablesGk)}
              onContextMenu={(e) => openMenu(e, { kind: "tables-group", connId: c.id, database: db })}
            >
              <span className="tree-caret">{tablesGExpanded ? "▾" : "▸"}</span>
              <span className="tree-icon">📁</span>
              <span className="tree-label">{t("tree.tables")}</span>
            </div>,
          );
          if (tablesGExpanded) {
            for (const t of tables[tablesGk] ?? []) {
              const tk = tblKey(c.id, db, t.name);
              const tExpanded = expanded.has(tk);
              nodes.push(
                <div
                  key={tk}
                  className="tree-node"
                  style={{ paddingLeft: 58 }}
                  onClick={() => {
                    toggle(tk);
                    onOpenTable(c.id, db, t.name);
                  }}
                  onContextMenu={(e) =>
                    openMenu(e, { kind: "table", connId: c.id, database: db, name: t.name })
                  }
                >
                  <span className="tree-caret">{tExpanded ? "▾" : "▸"}</span>
                  <span className="tree-icon">▤</span>
                  <span className="tree-label ellipsis">{t.name}</span>
                </div>,
              );
              if (tExpanded) {
                for (const col of columns[tk] ?? []) {
                  nodes.push(
                    <div
                      key={keyOf({ kind: "column", connId: c.id, db, table: t.name, column: col.name })}
                      className="tree-node leaf"
                      style={{ paddingLeft: 76 }}
                    >
                      <span className="tree-icon">·</span>
                      <span className="tree-label ellipsis">{col.name}</span>
                      <span className="tree-col-type">{col.type_name}</span>
                    </div>,
                  );
                }
              }
            }
          }

          // Views group
          const viewsGk = viewsGroupKey(c.id, db);
          const viewsGExpanded = expanded.has(viewsGk);
          nodes.push(
            <div
              key={viewsGk}
              className="tree-node"
              style={{ paddingLeft: 40 }}
              onClick={() => toggle(viewsGk)}
              onContextMenu={(e) => openMenu(e, { kind: "views-group", connId: c.id, database: db })}
            >
              <span className="tree-caret">{viewsGExpanded ? "▾" : "▸"}</span>
              <span className="tree-icon">📁</span>
              <span className="tree-label">{t("tree.views")}</span>
            </div>,
          );
          if (viewsGExpanded) {
            for (const v of views[viewsGk] ?? []) {
              const vk = viewKey(c.id, db, v.name);
              nodes.push(
                <div
                  key={vk}
                  className="tree-node leaf"
                  style={{ paddingLeft: 58 }}
                  onContextMenu={(e) =>
                    openMenu(e, { kind: "view", connId: c.id, database: db, name: v.name })
                  }
                >
                  <span className="tree-icon">👁</span>
                  <span className="tree-label ellipsis">{v.name}</span>
                </div>,
              );
            }
          }

          // Functions group
          const functionsGk = functionsGroupKey(c.id, db);
          const functionsGExpanded = expanded.has(functionsGk);
          nodes.push(
            <div
              key={functionsGk}
              className="tree-node"
              style={{ paddingLeft: 40 }}
              onClick={() => toggle(functionsGk)}
              onContextMenu={(e) => openMenu(e, { kind: "functions-group", connId: c.id, database: db })}
            >
              <span className="tree-caret">{functionsGExpanded ? "▾" : "▸"}</span>
              <span className="tree-icon">📁</span>
              <span className="tree-label">{t("tree.functions")}</span>
            </div>,
          );
          if (functionsGExpanded) {
            for (const f of functions[functionsGk] ?? []) {
              const fk = funcKey(c.id, db, f.name);
              nodes.push(
                <div
                  key={fk}
                  className="tree-node leaf"
                  style={{ paddingLeft: 58 }}
                  onContextMenu={(e) =>
                    openMenu(e, { kind: "function", connId: c.id, database: db, name: f.name })
                  }
                >
                  <span className="tree-icon">ƒ</span>
                  <span className="tree-label ellipsis">{f.name}</span>
                </div>,
              );
            }
          }

          // Procedures group
          const proceduresGk = proceduresGroupKey(c.id, db);
          const proceduresGExpanded = expanded.has(proceduresGk);
          nodes.push(
            <div
              key={proceduresGk}
              className="tree-node"
              style={{ paddingLeft: 40 }}
              onClick={() => toggle(proceduresGk)}
              onContextMenu={(e) => openMenu(e, { kind: "procedures-group", connId: c.id, database: db })}
            >
              <span className="tree-caret">{proceduresGExpanded ? "▾" : "▸"}</span>
              <span className="tree-icon">📁</span>
              <span className="tree-label">{t("tree.procedures")}</span>
            </div>,
          );
          if (proceduresGExpanded) {
            for (const p of procedures[proceduresGk] ?? []) {
              const pk = procKey(c.id, db, p.name);
              nodes.push(
                <div
                  key={pk}
                  className="tree-node leaf"
                  style={{ paddingLeft: 58 }}
                  onContextMenu={(e) =>
                    openMenu(e, { kind: "procedure", connId: c.id, database: db, name: p.name })
                  }
                >
                  <span className="tree-icon">⚙</span>
                  <span className="tree-label ellipsis">{p.name}</span>
                </div>,
              );
            }
          }

          // Triggers group
          const triggersGk = triggersGroupKey(c.id, db);
          const triggersGExpanded = expanded.has(triggersGk);
          nodes.push(
            <div
              key={triggersGk}
              className="tree-node"
              style={{ paddingLeft: 40 }}
              onClick={() => toggle(triggersGk)}
            >
              <span className="tree-caret">{triggersGExpanded ? "▾" : "▸"}</span>
              <span className="tree-icon">📁</span>
              <span className="tree-label">{t("tree.triggers")}</span>
            </div>,
          );
          if (triggersGExpanded) {
            for (const tr of triggers[triggersGk] ?? []) {
              const trk = triggerKey(c.id, db, tr.name);
              nodes.push(
                <div
                  key={trk}
                  className="tree-node leaf"
                  style={{ paddingLeft: 58 }}
                  onContextMenu={(e) =>
                    openMenu(e, { kind: "trigger", connId: c.id, database: db, name: tr.name })
                  }
                >
                  <span className="tree-icon">⚡</span>
                  <span className="tree-label ellipsis">{tr.name}</span>
                </div>,
              );
            }
          }
        }
      }
    }
  }

  const menuItems: { label: string; action: () => void }[] = [];
  if (menu) {
    const node = menu.node;
    if (node.kind === "connection") {
      // R11: 根据连接状态显示不同菜单
      const conn = connections.find((c) => c.id === node.connId);
      const isActive = activeId === node.connId;
      
      if (isActive) {
        // 已连接：显示"断开连接"
        menuItems.push({ label: t("tree.menu.disconnect"), action: () => onDisconnect(node.connId) });
      } else if (conn?.config_id) {
        // 未连接但有 config_id：显示"打开连接"
        menuItems.push({ label: t("tree.menu.reconnect"), action: () => onReconnect(conn.config_id!) });
      }
      
      // #73: 如果有 config_id，添加"删除连接"选项
      if (conn?.config_id) {
        menuItems.push({ label: t("tree.menu.deleteConnection"), action: () => onDeleteConnection(conn.config_id!) });
      }
    } else if (node.kind === "database") {
      // #75: 数据库节点 - 展开各个分组
      menuItems.push({
        label: t("tree.menu.viewTables"),
        action: () => toggle(tablesGroupKey(node.connId, node.name)),
      });
      menuItems.push({
        label: t("tree.menu.viewViews"),
        action: () => toggle(viewsGroupKey(node.connId, node.name)),
      });
      menuItems.push({
        label: t("tree.menu.viewFunctions"),
        action: () => toggle(functionsGroupKey(node.connId, node.name)),
      });
      menuItems.push({
        label: t("tree.menu.viewProcedures"),
        action: () => toggle(proceduresGroupKey(node.connId, node.name)),
      });
      menuItems.push({ label: t("tree.menu.dropDatabase"), action: () => handleDropDatabase(node) });
    } else if (node.kind === "table") {
      menuItems.push({
        label: t("tree.menu.queryData"),
        action: () => onOpenTable(node.connId, node.database, node.name),
      });
      menuItems.push({
        label: t("tree.menu.showDDL"),
        action: () => onShowDDL(node.connId, node.database, node.name),
      });
      menuItems.push({
        label: t("tree.menu.editStructure"),
        action: () => onEditStructure(node.connId, node.database, node.name),
      });
      menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
      menuItems.push({ label: t("tree.menu.rename"), action: () => handleRenameTable(node) });
      menuItems.push({ label: t("tree.menu.truncateTable"), action: () => handleTruncateTable(node) });
      menuItems.push({ label: t("tree.menu.dropTable"), action: () => handleDropTable(node) });
    } else if (node.kind === "view") {
      menuItems.push({
        label: t("tree.menu.showDDL"),
        action: () => onShowObjectDDL(node.connId, node.database, node.name, "VIEW"),
      });
      menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
      menuItems.push({ label: t("tree.menu.dropView"), action: () => handleDropView(node) });
    } else if (node.kind === "function") {
      menuItems.push({
        label: t("tree.menu.showDDL"),
        action: () => onShowObjectDDL(node.connId, node.database, node.name, "FUNCTION"),
      });
      menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
      menuItems.push({ label: t("tree.menu.dropFunction"), action: () => handleDropFunction(node) });
    } else if (node.kind === "procedure") {
      menuItems.push({
        label: t("tree.menu.showDDL"),
        action: () => onShowObjectDDL(node.connId, node.database, node.name, "PROCEDURE"),
      });
      menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
      menuItems.push({ label: t("tree.menu.dropProcedure"), action: () => handleDropProcedure(node) });
    } else if (node.kind === "trigger") {
      menuItems.push({
        label: t("tree.menu.showDDL"),
        action: () => onShowObjectDDL(node.connId, node.database, node.name, "TRIGGER"),
      });
      menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
      menuItems.push({ label: t("tree.menu.dropTrigger"), action: () => handleDropTrigger(node) });
    } else if (node.kind === "tables-group") {
      // #75: 表分组 - 新建表（可视化对话框）+ 创建表 (SQL)
      menuItems.push({
        label: t("tree.menu.createTable"),
        action: () => setCreateTable({ connId: node.connId, database: node.database }),
      });
      menuItems.push({
        label: t("tree.menu.createTableSQL"),
        action: () => {
          const template = `-- 创建表\nCREATE TABLE \`${node.database}\`.\`table_name\` (\n  \`id\` INT PRIMARY KEY AUTO_INCREMENT,\n  \`name\` VARCHAR(255) NOT NULL,\n  \`created_at\` TIMESTAMP DEFAULT CURRENT_TIMESTAMP\n);\n`;
          onInsertTemplate(node.connId, template);
        },
      });
    } else if (node.kind === "views-group") {
      // #75: 视图分组 - 创建视图
      menuItems.push({
        label: t("tree.menu.createView"),
        action: () => {
          const template = `-- 创建视图\nCREATE VIEW \`${node.database}\`.\`view_name\` AS\nSELECT * FROM \`${node.database}\`.\`table_name\`;\n`;
          onInsertTemplate(node.connId, template);
        },
      });
    } else if (node.kind === "functions-group") {
      // #75: 函数分组 - 创建函数
      menuItems.push({
        label: t("tree.menu.createFunction"),
        action: () => {
          const template = `-- 创建函数\nCREATE FUNCTION \`${node.database}\`.\`function_name\`() RETURNS INT\nDETERMINISTIC\nBEGIN\n  RETURN 1;\nEND;\n`;
          onInsertTemplate(node.connId, template);
        },
      });
    } else if (node.kind === "procedures-group") {
      // #75: 存储过程分组 - 创建存储过程
      menuItems.push({
        label: t("tree.menu.createProcedure"),
        action: () => {
          const template = `-- 创建存储过程\nCREATE PROCEDURE \`${node.database}\`.\`procedure_name\`()\nBEGIN\n  SELECT 'Hello';\nEND;\n`;
          onInsertTemplate(node.connId, template);
        },
      });
    }
  }

  return (
    <div className="schema-tree">
      <div className="tree-list">{nodes}</div>
      {nodes.length === 0 && <div className="empty">{t("tree.empty.noConnections")}</div>}
      {status && <div className="tree-status">{status}</div>}

      {menu && (
        <>
          <div className="ctx-overlay" onClick={() => setMenu(null)} />
          <div className="ctx-menu" style={{ left: menu.x, top: menu.y }}>
            {menuItems.map((m) => (
              <div
                key={m.label}
                className="ctx-item"
                onClick={() => {
                  setMenu(null);
                  m.action();
                }}
              >
                {m.label}
              </div>
            ))}
          </div>
        </>
      )}

      {createTable && (
        <CreateTableDialog
          connId={createTable.connId}
          database={createTable.database}
          onDone={() => {
            const c = createTable;
            setCreateTable(null);
            loadChildren(dbKey(c.connId, c.database));
          }}
          onClose={() => setCreateTable(null)}
        />
      )}
    </div>
  );
}
