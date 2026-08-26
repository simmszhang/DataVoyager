import { ReactElement, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ColumnInfo, ConnectionSummary, TableInfo } from "../api";
import { errToString } from "../i18n";
import CreateTableDialog from "./CreateTableDialog";

type MenuNode =
  | { kind: "connection"; connId: number }
  | { kind: "database"; connId: number; name: string }
  | { kind: "table"; connId: number; database: string; name: string };

interface Props {
  connections: ConnectionSummary[];
  activeId: number | null;
  onSelectConnection: (id: number) => void;
  onDisconnect: (id: number) => void;
  onReconnect: (configId: string) => void; // R11: 重连已保存的连接
  onOpenTable: (connId: number, database: string, table: string) => void;
  onShowDDL: (connId: number, database: string, table: string) => void;
  onEditStructure: (connId: number, database: string, table: string) => void;
}

/// 结构化节点 key：JSON 编码，避免 `:` 拼接/切分在库表名含分隔符时失效（defect #3）。
type NodeKey =
  | { kind: "conn"; connId: number }
  | { kind: "db"; connId: number; db: string }
  | { kind: "table"; connId: number; db: string; table: string }
  | { kind: "column"; connId: number; db: string; table: string; column: string };

const keyOf = (k: NodeKey): string => JSON.stringify(k);
const parseKey = (key: string): NodeKey => JSON.parse(key);

const connKey = (id: number) => keyOf({ kind: "conn", connId: id });
const dbKey = (id: number, db: string) => keyOf({ kind: "db", connId: id, db });
const tblKey = (id: number, db: string, t: string) =>
  keyOf({ kind: "table", connId: id, db, table: t });

export default function SchemaTree({
  connections,
  activeId,
  onSelectConnection,
  onDisconnect,
  onReconnect,
  onOpenTable,
  onShowDDL,
  onEditStructure,
}: Props) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [dbs, setDbs] = useState<Record<string, string[]>>({});
  const [tables, setTables] = useState<Record<string, TableInfo[]>>({});
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
        const list = await api.listTables(node.connId, node.db);
        setTables((p) => ({ ...p, [key]: list }));
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
      .then(() => loadChildren(dbKey(n.connId, n.database)))
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
          for (const t of tables[dk] ?? []) {
            const tk = tblKey(c.id, db, t.name);
            const tExpanded = expanded.has(tk);
            nodes.push(
              <div
                key={tk}
                className="tree-node"
                style={{ paddingLeft: 40 }}
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
                    style={{ paddingLeft: 58 }}
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
    } else if (node.kind === "database") {
      menuItems.push({
        label: t("tree.menu.createTable"),
        action: () => setCreateTable({ connId: node.connId, database: node.name }),
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
