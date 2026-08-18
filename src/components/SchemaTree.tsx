import { ReactElement, useState } from "react";
import { api, ColumnInfo, ConnectionSummary, TableInfo } from "../api";
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
  onOpenTable: (connId: number, database: string, table: string) => void;
}

const connKey = (id: number) => `c:${id}`;
const dbKey = (id: number, db: string) => `d:${id}:${db}`;
const tblKey = (id: number, db: string, t: string) => `t:${id}:${db}:${t}`;

export default function SchemaTree({
  connections,
  activeId,
  onSelectConnection,
  onDisconnect,
  onOpenTable,
}: Props) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [dbs, setDbs] = useState<Record<string, string[]>>({});
  const [tables, setTables] = useState<Record<string, TableInfo[]>>({});
  const [columns, setColumns] = useState<Record<string, ColumnInfo[]>>({});
  const [menu, setMenu] = useState<{ x: number; y: number; node: MenuNode } | null>(null);
  const [createTable, setCreateTable] = useState<{ connId: number; database: string } | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  async function loadChildren(key: string) {
    const [kind, ...rest] = key.split(":");
    try {
      if (kind === "c") {
        const connId = Number(rest[0]);
        const list = await api.listDatabases(connId);
        setDbs((p) => ({ ...p, [key]: list }));
      } else if (kind === "d") {
        const connId = Number(rest[0]);
        const db = rest.slice(1).join(":");
        const list = await api.listTables(connId, db);
        setTables((p) => ({ ...p, [key]: list }));
      } else if (kind === "t") {
        const connId = Number(rest[0]);
        const db = rest[1];
        const t = rest.slice(2).join(":");
        const list = await api.listColumns(connId, db, t);
        setColumns((p) => ({ ...p, [key]: list }));
      }
    } catch (e) {
      setStatus(String(e));
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
    const newName = window.prompt("新表名", n.name);
    if (!newName || newName === n.name) return;
    api
      .renameTable(n.connId, n.database, n.name, newName, true)
      .then(() => loadChildren(dbKey(n.connId, n.database)))
      .catch((e) => setStatus(String(e)));
  }

  function handleDropDatabase(n: { connId: number; name: string }) {
    if (!window.confirm(`确定删除数据库「${n.name}」？此操作不可撤销。`)) return;
    api
      .dropDatabase(n.connId, n.name, true)
      .then(() => loadChildren(connKey(n.connId)))
      .catch((e) => setStatus(String(e)));
  }

  function handleDropTable(n: { connId: number; database: string; name: string }) {
    if (!window.confirm(`确定删除表「${n.name}」？此操作不可撤销。`)) return;
    api
      .dropTable(n.connId, n.database, n.name, true)
      .then(() => loadChildren(dbKey(n.connId, n.database)))
      .catch((e) => setStatus(String(e)));
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
                  <div key={`${tk}.${col.name}`} className="tree-node leaf" style={{ paddingLeft: 58 }}>
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
      menuItems.push({ label: "断开连接", action: () => onDisconnect(node.connId) });
    } else if (node.kind === "database") {
      menuItems.push({
        label: "新建表",
        action: () => setCreateTable({ connId: node.connId, database: node.name }),
      });
      menuItems.push({ label: "删除数据库", action: () => handleDropDatabase(node) });
    } else if (node.kind === "table") {
      menuItems.push({
        label: "查询数据",
        action: () => onOpenTable(node.connId, node.database, node.name),
      });
      menuItems.push({ label: "重命名", action: () => handleRenameTable(node) });
      menuItems.push({ label: "删除表", action: () => handleDropTable(node) });
    }
  }

  return (
    <div className="schema-tree">
      <div className="tree-list">{nodes}</div>
      {nodes.length === 0 && <div className="empty">暂无连接</div>}
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
