# 右键菜单完善设计

## 一、需求背景

**现状问题**：
1. 浏览器原生右键菜单与自定义菜单共存，体验割裂
2. Schema 树只有 3 种节点（连接/数据库/表）有右键菜单，视图/函数/存储过程/触发器/列/分类节点均无
3. 视图删除用错 SQL（`DROP TABLE` 而非 `DROP VIEW`）导致执行失败
4. ResultsGrid 无右键菜单（复制单元格/行、生成 INSERT 等操作缺失）

**目标**：
1. 全局屏蔽浏览器原生右键菜单，统一自定义菜单体验
2. Schema 树各级节点补齐右键菜单功能
3. 修复视图删除 bug
4. ResultsGrid 新增右键菜单（复制、置 NULL、生成 INSERT）
5. 后端新增 `drop_view`/`drop_routine`/`drop_trigger`/`truncate_table`/`build_insert_sql` 五个命令

## 二、方案选择

### 方案 A：三模块完全并行（已选）

**结构**：
- **模块 1**（全局屏蔽）：独立最小改动，先合并提供基础
- **模块 2**（Schema 树）：拆 P2a（后端 4 个 DDL 命令）、P2b（前端菜单扩展）两子任务并行
- **模块 3**（ResultsGrid）：依赖模块 1，并行开发

**理由**：
- 模块 1 极小（2 行代码），立即完成为后续提供基础
- 模块 2 前后端可并行，P2b 的 UI 结构可先行，命令到位后接线
- 三模块功能正交，合并冲突风险低
- 最大化并行效率

## 三、详细设计

### 3.1 模块 1：全局右键屏蔽

**目标**：屏蔽浏览器原生右键菜单，为自定义菜单让路。

**改动**：

**`src/App.tsx`（第 457 行）**：
```tsx
// 最外层 div 加 onContextMenu 全局屏蔽
<div className="app" onContextMenu={(e) => e.preventDefault()}>
```

**`src/components/QueryEditor.tsx`（第 70 行）**：
```tsx
// 编辑器容器保留原生菜单（复制/粘贴）
<div className="editor" onContextMenu={(e) => e.stopPropagation()}>
  <CodeMirror ... />
</div>
```

**验收标准**：
- ✓ 空白区域右键无原生菜单
- ✓ Schema 树节点右键弹自定义菜单（现有行为保持）
- ✓ CodeMirror 编辑器内右键仍可复制/粘贴
- ✓ 未来新增的自定义菜单不受影响

---

### 3.2 模块 2：Schema 树右键菜单完善

#### P2a：后端新增命令

**目标**：新增 `drop_view`/`drop_routine`/`drop_trigger`/`truncate_table` 四个 DDL 命令，接入现有危险确认流程。

##### 1. `crates/dby-core/src/ddl.rs`

新增四个 SQL 生成函数（模仿 `build_drop_table`）：

```rust
/// 生成 DROP VIEW 语句
pub fn build_drop_view(dialect: &dyn Dialect, name: &str) -> String {
    format!("DROP VIEW {};", dialect.quote_identifier(name))
}

/// 生成 DROP FUNCTION/PROCEDURE 语句
pub fn build_drop_routine(dialect: &dyn Dialect, kind: &str, name: &str) -> String {
    // kind: "FUNCTION" | "PROCEDURE"
    format!("DROP {} {};", kind, dialect.quote_identifier(name))
}

/// 生成 DROP TRIGGER 语句
pub fn build_drop_trigger(dialect: &dyn Dialect, name: &str) -> String {
    format!("DROP TRIGGER {};", dialect.quote_identifier(name))
}

/// 生成 TRUNCATE TABLE 语句
pub fn build_truncate_table(dialect: &dyn Dialect, name: &str) -> String {
    format!("TRUNCATE TABLE {};", dialect.quote_identifier(name))
}
```

**单元测试**（追加到 `#[cfg(test)] mod tests`）：

```rust
#[test]
fn drop_view_and_routine() {
    let d = MySQLDialect;
    assert_eq!(build_drop_view(&d, "v1"), "DROP VIEW `v1`;");
    assert_eq!(
        build_drop_routine(&d, "FUNCTION", "fn1"),
        "DROP FUNCTION `fn1`;"
    );
    assert_eq!(
        build_drop_routine(&d, "PROCEDURE", "sp1"),
        "DROP PROCEDURE `sp1`;"
    );
    assert_eq!(build_drop_trigger(&d, "trg1"), "DROP TRIGGER `trg1`;");
    assert_eq!(build_truncate_table(&d, "t1"), "TRUNCATE TABLE `t1`;");
}
```

##### 2. `src-tauri/src/commands.rs`

新增四个 Tauri 命令（模仿 `drop_table:1322-1333` 的模式，走 `run_ddl` + `guard_dangerous`）：

```rust
#[tauri::command]
pub async fn drop_view(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_view(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn drop_routine(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    kind: String, // "FUNCTION" | "PROCEDURE"
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_routine(driver.dialect(), &kind, &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn drop_trigger(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_drop_trigger(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}

#[tauri::command]
pub async fn truncate_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    name: String,
    confirmed: bool,
) -> Result<QueryOutput> {
    let driver = driver_for(state.inner(), id).await?;
    let sql = dby_core::ddl::build_truncate_table(driver.dialect(), &name);
    guard_dangerous(&sql, confirmed)?;
    run_ddl(state.inner(), id, Some(database), sql).await
}
```

**关键特性**：
- 走 `run_ddl` 自动记历史（`SqlOrigin::SchemaEdit`）
- 走 `guard_dangerous` 危险 SQL 需前端传 `confirmed: true`，否则拒绝执行
- 与现有 `drop_table`/`rename_table` 完全同构

##### 3. `src-tauri/src/lib.rs`

注册四个命令（追加到 `invoke_handler!` 宏列表）：

```rust
.invoke_handler(tauri::generate_handler![
    // ...现有命令...
    drop_view,
    drop_routine,
    drop_trigger,
    truncate_table,
])
```

##### 4. `src/api.ts`

补充前端类型和 API 封装：

```typescript
export const api = {
  // ...现有方法...
  
  dropView: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_view", { id, database, name, confirmed }),
  
  dropRoutine: (id: number, database: string, kind: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_routine", { id, database, kind, name, confirmed }),
  
  dropTrigger: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_trigger", { id, database, name, confirmed }),
  
  truncateTable: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("truncate_table", { id, database, name, confirmed }),
};
```

**文件清单**：
- `crates/dby-core/src/ddl.rs` （新增 4 函数 + 测试）
- `src-tauri/src/commands.rs` （新增 4 命令）
- `src-tauri/src/lib.rs` （注册 4 命令）
- `src/api.ts` （新增 4 方法）

---

#### P2b：前端 SchemaTree 菜单扩展

**目标**：为分类节点、视图、函数/存储过程、触发器、列节点补齐右键菜单；修复视图删除 bug。

**改动文件**：`src/components/SchemaTree.tsx`

##### 1. 扩展 `MenuNode` 类型（第 7-10 行）

```tsx
type MenuNode =
  | { kind: "connection"; configId: string; connId: number | null }
  | { kind: "database"; configId: string; connId: number; name: string }
  | { kind: "category"; configId: string; connId: number; database: string; category: Category }
  | { kind: "table"; configId: string; connId: number; database: string; name: string }
  | { kind: "view"; configId: string; connId: number; database: string; name: string }
  | { kind: "routine"; configId: string; connId: number; database: string; routineKind: string; name: string }
  | { kind: "trigger"; configId: string; connId: number; database: string; name: string }
  | { kind: "column"; configId: string; connId: number; database: string; table: string; name: string; typeName: string };
```

**新增字段说明**：
- `category`：分类节点携带 `Category` 枚举值（tables/views/functions/procedures/triggers）
- `routineKind`：函数/存储过程节点携带原始 `routine.kind`（"FUNCTION"/"PROCEDURE"），后端删除时需要
- `typeName`：列节点携带列类型名，用于"复制列定义"菜单项

##### 2. 各级节点补 `onContextMenu`

**分类节点**（第 236-246 行渲染处）：
```tsx
<div
  key={catKey}
  className="tree-node category"
  style={{ paddingLeft: 40 }}
  onClick={() => toggle(catKey)}
  onContextMenu={(e) =>
    openMenu(e, { kind: "category", configId: s.id, connId: connId!, database: db, category: cat })
  }
>
```

**表/视图节点区分**（第 257-283 行，按 `cat` 区分 `kind`）：
```tsx
const filtered = list.filter((tbl) => {
  const kind = tbl.kind?.toLowerCase();
  if (cat === "tables") return !kind || kind === "base table";
  if (cat === "views") return kind === "view";
  return false;
});
for (const tbl of filtered) {
  const tk = tblKey(s.id, connId!, db, tbl.name);
  const tExpanded = expanded.has(tk);
  nodes.push(
    <div
      key={tk}
      className="tree-node"
      style={{ paddingLeft: 58 }}
      onClick={() => {
        toggle(tk);
        onOpenTable(connId!, db, tbl.name);
      }}
      onContextMenu={(e) =>
        openMenu(e, {
          kind: cat === "views" ? "view" : "table",  // 关键：区分 view/table
          configId: s.id,
          connId: connId!,
          database: db,
          name: tbl.name,
        })
      }
    >
```

**函数/存储过程节点**（第 315-326 行）：
```tsx
for (const routine of filtered) {
  nodes.push(
    <div
      key={`${catKey}:${routine.name}`}
      className="tree-node leaf"
      style={{ paddingLeft: 58 }}
      onContextMenu={(e) =>
        openMenu(e, {
          kind: "routine",
          configId: s.id,
          connId: connId!,
          database: db,
          routineKind: routine.kind,  // "FUNCTION" | "PROCEDURE"
          name: routine.name,
        })
      }
    >
```

**触发器节点**（第 329-343 行）：
```tsx
for (const trigger of list) {
  nodes.push(
    <div
      key={`${catKey}:${trigger.name}`}
      className="tree-node leaf"
      style={{ paddingLeft: 58 }}
      onContextMenu={(e) =>
        openMenu(e, {
          kind: "trigger",
          configId: s.id,
          connId: connId!,
          database: db,
          name: trigger.name,
        })
      }
    >
```

**列节点**（第 286-304 行）：
```tsx
for (const col of columns[tk] ?? []) {
  nodes.push(
    <div
      key={keyOf({ kind: "column", configId: s.id, connId: connId!, db, table: tbl.name, column: col.name })}
      className="tree-node leaf"
      style={{ paddingLeft: 76 }}
      onContextMenu={(e) =>
        openMenu(e, {
          kind: "column",
          configId: s.id,
          connId: connId!,
          database: db,
          table: tbl.name,
          name: col.name,
          typeName: col.type_name,
        })
      }
    >
```

##### 3. 新增 handler 函数

在现有 `handleRenameTable`/`handleDropDatabase`/`handleDropTable` 后追加：

```tsx
function handleDropView(n: { configId: string; connId: number; database: string; name: string }) {
  if (!window.confirm(t("tree.dropViewConfirm", { name: n.name }))) return;
  api
    .dropView(n.connId, n.database, n.name, true)
    .then(() => loadChildren(categoryKey(n.configId, n.connId, n.database, "views")))
    .catch((e) => setStatus(errToString(e)));
}

function handleDropRoutine(n: { configId: string; connId: number; database: string; routineKind: string; name: string }) {
  if (!window.confirm(t("tree.dropRoutineConfirm", { name: n.name }))) return;
  const cat: Category = n.routineKind.toLowerCase() === "function" ? "functions" : "procedures";
  api
    .dropRoutine(n.connId, n.database, n.routineKind, n.name, true)
    .then(() => loadChildren(categoryKey(n.configId, n.connId, n.database, cat)))
    .catch((e) => setStatus(errToString(e)));
}

function handleDropTrigger(n: { configId: string; connId: number; database: string; name: string }) {
  if (!window.confirm(t("tree.dropTriggerConfirm", { name: n.name }))) return;
  api
    .dropTrigger(n.connId, n.database, n.name, true)
    .then(() => loadChildren(categoryKey(n.configId, n.connId, n.database, "triggers")))
    .catch((e) => setStatus(errToString(e)));
}

function handleTruncateTable(n: { configId: string; connId: number; database: string; name: string }) {
  if (!window.confirm(t("tree.truncateConfirm", { name: n.name }))) return;
  api
    .truncateTable(n.connId, n.database, n.name, true)
    .then(() => setStatus(t("tree.truncateSuccess")))
    .catch((e) => setStatus(errToString(e)));
}

function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}
```

**关键点**：
- `handleDropView` 刷新 `"views"` 分类节点（修复 bug：不再走 `handleDropTable`）
- `handleDropRoutine` 根据 `routineKind` 判断刷新 `"functions"` 还是 `"procedures"`
- `handleTruncateTable` 成功后只提示，不刷新（TRUNCATE 不改变表结构）

##### 4. 扩展菜单项构建逻辑（第 352-386 行）

现有 `connection`/`database` 分支保持，新增 5 个分支：

```tsx
const menuItems: { label: string; action: () => void }[] = [];
if (menu) {
  const node = menu.node;
  if (node.kind === "connection") {
    // ...现有逻辑不变...
  } else if (node.kind === "database") {
    menuItems.push({ label: t("tree.menu.createTable"), action: () => setCreateTable(...) });
    menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });  // 新增
    menuItems.push({ label: t("tree.menu.dropDatabase"), action: () => handleDropDatabase(node) });
  } else if (node.kind === "category") {
    menuItems.push({
      label: t("tree.menu.refresh"),
      action: () => loadChildren(categoryKey(node.configId, node.connId, node.database, node.category)),
    });
  } else if (node.kind === "table") {
    menuItems.push({ label: t("tree.menu.queryData"), action: () => onOpenTable(node.connId, node.database, node.name) });
    menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
    menuItems.push({ label: t("tree.menu.rename"), action: () => handleRenameTable(node) });
    menuItems.push({ label: t("tree.menu.truncateTable"), action: () => handleTruncateTable(node) });
    menuItems.push({ label: t("tree.menu.dropTable"), action: () => handleDropTable(node) });
  } else if (node.kind === "view") {
    menuItems.push({ label: t("tree.menu.queryData"), action: () => onOpenTable(node.connId, node.database, node.name) });
    menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
    menuItems.push({ label: t("tree.menu.dropView"), action: () => handleDropView(node) });
  } else if (node.kind === "routine") {
    menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
    menuItems.push({ label: t("tree.menu.dropRoutine"), action: () => handleDropRoutine(node) });
  } else if (node.kind === "trigger") {
    menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
    menuItems.push({ label: t("tree.menu.dropTrigger"), action: () => handleDropTrigger(node) });
  } else if (node.kind === "column") {
    menuItems.push({ label: t("tree.menu.copyColumnName"), action: () => copyToClipboard(node.name) });
    menuItems.push({ label: t("tree.menu.copyColumnDef"), action: () => copyToClipboard(`${node.name} ${node.typeName}`) });
  }
}
```

**菜单项总结**：
- **分类节点**：刷新
- **数据库节点**：创建表、**复制库名（新增）**、删除库
- **表节点**：查询数据、复制名称、重命名、**清空表（新增）**、删除表
- **视图节点**：查询数据、复制名称、**删除视图（修复 bug）**
- **函数/存储过程节点**：复制名称、删除
- **触发器节点**：复制名称、删除
- **列节点**：复制列名、复制列定义（`列名 类型`）

##### 5. i18n 文案补充

需要在 `src/locales/zh-CN.json` 和 `src/locales/en-US.json` 补充以下键（模仿现有 `tree.*` 风格）：

```json
{
  "tree": {
    "menu": {
      "refresh": "刷新",
      "copyName": "复制名称",
      "truncateTable": "清空表",
      "dropView": "删除视图",
      "dropRoutine": "删除",
      "dropTrigger": "删除触发器",
      "copyColumnName": "复制列名",
      "copyColumnDef": "复制列定义"
    },
    "dropViewConfirm": "确定要删除视图 {{name}} 吗？",
    "dropRoutineConfirm": "确定要删除 {{name}} 吗？",
    "dropTriggerConfirm": "确定要删除触发器 {{name}} 吗？",
    "truncateConfirm": "确定要清空表 {{name}} 的所有数据吗？此操作不可撤销！",
    "truncateSuccess": "表已清空"
  }
}
```

英文对应：
```json
{
  "tree": {
    "menu": {
      "refresh": "Refresh",
      "copyName": "Copy Name",
      "truncateTable": "Truncate Table",
      "dropView": "Drop View",
      "dropRoutine": "Drop",
      "dropTrigger": "Drop Trigger",
      "copyColumnName": "Copy Column Name",
      "copyColumnDef": "Copy Column Definition"
    },
    "dropViewConfirm": "Are you sure you want to drop view {{name}}?",
    "dropRoutineConfirm": "Are you sure you want to drop {{name}}?",
    "dropTriggerConfirm": "Are you sure you want to drop trigger {{name}}?",
    "truncateConfirm": "Are you sure you want to truncate table {{name}}? This will delete all data and cannot be undone!",
    "truncateSuccess": "Table truncated successfully"
  }
}
```

**文件清单**：
- `src/components/SchemaTree.tsx`（主要改动）
- `src/locales/zh-CN.json`（新增文案）
- `src/locales/en-US.json`（新增文案）

---

### 3.3 模块 3：ResultsGrid 右键菜单

**目标**：单元格/行级右键菜单：复制单元格值、复制行 JSON、复制为 INSERT、设为 NULL。

#### 后端：`build_insert_sql` 命令

**`src-tauri/src/commands.rs`** 新增命令（复用现有 `build_edit_sql` 模式，包装 `dby_core::edit::build_insert`）：

```rust
#[tauri::command]
pub async fn build_insert_sql(
    state: State<'_, Arc<AppState>>,
    id: u64,
    table: String,
    cells: Vec<(String, ColumnType, String)>, // 复用 EditCell 三元组
) -> Result<String> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    
    let parsed = parse_cells(&cells)?; // 复用现有 parse_cells
    let columns: Vec<String> = parsed.iter().map(|(n, _)| n.clone()).collect();
    let values: Vec<dby_core::value::Value> = parsed.into_iter().map(|(_, v)| v).collect();
    
    Ok(dby_core::edit::build_insert(
        driver.dialect(),
        &table,
        &columns,
        &values,
    ))
}
```

**关键点**：
- 复用 `parse_cells`（将 `EditCell` 解析为 `Value`）
- 调用 `dby_core::edit::build_insert`（已存在，无需新增 ddl 函数）
- 走方言引号，遵循 defects #4 已修复的"SQL 生成必须走 Dialect"原则

**`src-tauri/src/lib.rs`**：注册 `build_insert_sql` 命令（追加到 `invoke_handler!`）。

**`src/api.ts`**：
```typescript
buildInsertSql: (id: number, table: string, cells: EditCell[]) =>
  invoke<string>("build_insert_sql", { id, table, cells }),
```

#### 前端：ResultsGrid 菜单 UI

**`src/components/ResultsGrid.tsx`** 改动：

##### 1. 新增状态和 Props

```tsx
interface Props {
  result: StreamResult;
  onEditCell?: (rowIndex: number, colIndex: number, newValue: string) => void;
  tableName?: string | null;      // 新增：当前表名，用于生成 INSERT
  connId?: number | null;          // 新增：连接 id，调 build_insert_sql 需要
}

export default function ResultsGrid({ result, onEditCell, tableName, connId }: Props) {
  // ...现有状态...
  const [menu, setMenu] = useState<{ x: number; y: number; row: number; col: number } | null>(null);
```

##### 2. 单元格加 `onContextMenu`

在单元格 `<div>` 上加（第 214-225 行附近，`grid-cell` 渲染处）：

```tsx
<div
  className="grid-cell"
  key={j}
  onDoubleClick={() => onEditCell && setEditing(...)}
  onContextMenu={(e) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, row: vi.index, col: j });
  }}
>
```

##### 3. 菜单项逻辑

```tsx
const menuItems: { label: string; action: () => void; disabled?: boolean }[] = [];
if (menu && result.columns) {
  const row = result.rows[menu.row];
  const cell = row[menu.col];
  const columns = result.columns;
  
  // 复制单元格值
  menuItems.push({
    label: t("grid.menu.copyCell"),
    action: () => {
      navigator.clipboard.writeText(displayCell(cell));
      setMenu(null);
    },
  });
  
  // 复制整行为 JSON
  menuItems.push({
    label: t("grid.menu.copyRowAsJson"),
    action: () => {
      const rowObj = Object.fromEntries(
        columns.map((c, i) => [c.name, displayCell(row[i])])
      );
      navigator.clipboard.writeText(JSON.stringify(rowObj, null, 2));
      setMenu(null);
    },
  });
  
  // 复制为 INSERT 语句（需要表名和连接 id）
  if (tableName && connId) {
    menuItems.push({
      label: t("grid.menu.copyAsInsert"),
      action: async () => {
        try {
          const cells: EditCell[] = columns.map((c, i) => [
            c.name,
            c.column_type ?? UNKNOWN_COLUMN_TYPE,
            displayCell(row[i]),
          ]);
          const sql = await api.buildInsertSql(connId, tableName, cells);
          navigator.clipboard.writeText(sql);
        } catch {
          // 失败静默（已写入剪贴板的保留）
        }
        setMenu(null);
      },
    });
  }
  
  // 设为 NULL（仅当可编辑时显示）
  if (onEditCell) {
    menuItems.push({
      label: t("grid.menu.setNull"),
      action: () => {
        onEditCell(menu.row, menu.col, "NULL");  // 复用现有 parse_value 的 NULL 语义
        setMenu(null);
      },
    });
  }
}
```

##### 4. 渲染菜单（追加到 return 的 JSX 末尾，模仿 SchemaTree）

```tsx
{menu && (
  <>
    <div className="ctx-overlay" onClick={() => setMenu(null)} />
    <div className="ctx-menu" style={{ left: menu.x, top: menu.y }}>
      {menuItems.map((m, i) => (
        <div
          key={i}
          className={`ctx-item ${m.disabled ? "disabled" : ""}`}
          onClick={() => !m.disabled && m.action()}
        >
          {m.label}
        </div>
      ))}
    </div>
  </>
)}
```

##### 5. App.tsx 传入新 Props

**`src/App.tsx`**（第 587 行，`<ResultsGrid>` 调用处）：

```tsx
<ResultsGrid
  result={ws.result}
  onEditCell={handleEditCell}
  tableName={ws.selectedTable}    // 新增
  connId={activeId}               // 新增
/>
```

##### 6. i18n 文案补充

```json
{
  "grid": {
    "menu": {
      "copyCell": "复制单元格",
      "copyRowAsJson": "复制行为 JSON",
      "copyAsInsert": "复制为 INSERT 语句",
      "setNull": "设为 NULL"
    }
  }
}
```

英文：
```json
{
  "grid": {
    "menu": {
      "copyCell": "Copy Cell",
      "copyRowAsJson": "Copy Row as JSON",
      "copyAsInsert": "Copy as INSERT",
      "setNull": "Set to NULL"
    }
  }
}
```

**文件清单**：
- `src-tauri/src/commands.rs` （新增 `build_insert_sql` 命令）
- `src-tauri/src/lib.rs` （注册命令）
- `src/api.ts` （新增方法）
- `src/components/ResultsGrid.tsx` （菜单 UI）
- `src/App.tsx` （传 props）
- `src/locales/zh-CN.json` / `en-US.json` （文案）

---

## 四、技术细节澄清

### 4.1 NULL 值语义

**现状**：`dby_core::edit::parse_value` 对任意列类型，输入 `"NULL"`（大小写不敏感、trim 后）都解析为 `Value::Null`，`quote_value` 将其渲染为 SQL `NULL`。

**结论**："设为 NULL"菜单项直接调 `onEditCell(row, col, "NULL")` 即可，不需要后端新增参数或前端特殊标记。

### 4.2 INSERT SQL 生成

**现状**：`dby_core::edit::build_insert` 已存在（`edit.rs:65-87`），接受 `dialect`、`table`、`columns`、`values`，返回带方言引号的 INSERT 语句。

**结论**：只需新增 Tauri 命令包一层（复用 `parse_cells` 将 `EditCell` 转 `Value`），不需要新增 dby-core ddl 函数。

### 4.3 视图删除 bug 根因

**现状**：`SchemaTree.tsx:257-306` 渲染 `cat === "views"` 的节点时，`onContextMenu` 传的是 `kind: "table"`，菜单项调 `handleDropTable` → `api.dropTable` → 后端生成 `DROP TABLE`，但 MySQL 视图必须用 `DROP VIEW`。

**修复**：`onContextMenu` 按 `cat` 区分传 `kind: "view"`，菜单项调新的 `handleDropView` → `api.dropView` → 后端生成 `DROP VIEW`。

---

## 五、验收标准

### 模块 1：全局右键屏蔽
- [ ] 空白区域右键无原生菜单
- [ ] Schema 树节点右键弹自定义菜单
- [ ] CodeMirror 编辑器内右键仍可复制/粘贴
- [ ] 其他区域（连接标签、结果表格）右键无原生菜单

### 模块 2：Schema 树
#### 后端（P2a）
- [ ] `cargo test -p dby-core` 通过（ddl.rs 新测试）
- [ ] `cargo clippy --workspace` 无警告
- [ ] `cargo fmt --all --check` 通过
- [ ] 手动测试：删除视图/函数/存储过程/触发器成功，危险确认流程正常

#### 前端（P2b）
- [ ] 分类节点右键"刷新"功能正常
- [ ] 数据库节点新增"复制库名"
- [ ] 表节点新增"清空表"、"复制名称"
- [ ] 视图节点右键可正常删除（走 `DROP VIEW`，不再报错）
- [ ] 函数/存储过程/触发器节点右键可删除
- [ ] 列节点右键可复制列名和列定义
- [ ] 所有确认对话框文案正确（中英双语）
- [ ] i18n check 通过（`node src/locales/check-keys.mjs`）

### 模块 3：ResultsGrid
- [ ] 单元格右键弹菜单
- [ ] "复制单元格"功能正常
- [ ] "复制行为 JSON"功能正常
- [ ] "复制为 INSERT"功能正常（有表名时可用，无表名时不显示）
- [ ] "设为 NULL"功能正常（可编辑时显示，置 NULL 后刷新正确）
- [ ] i18n check 通过

### 整体
- [ ] CI 门禁通过（cargo fmt/clippy/test + pnpm build）
- [ ] 无合并冲突
- [ ] 三模块功能互不影响

---

## 六、风险与依赖

### 风险
- **模块 2 前后端接口对齐**：前端 `api.dropView` 等调用必须等后端命令注册完成才能测试，建议 P2a 先合并。
- **i18n 文案遗漏**：新增 11 个中文键 + 11 个英文键，需跑 `check-keys.mjs` 验证无遗漏。
- **视图删除 bug 可能被其他代码依赖**：检查是否有其他地方调用 `handleDropTable` 处理视图（grep 确认无）。

### 依赖
- **模块 3 依赖模块 1**：全局屏蔽完成后 ResultsGrid 才能正常弹自定义菜单。
- **模块 2 前端依赖模块 2 后端**：P2b 可先做 UI 结构，但功能测试需等 P2a 合并。

---

## 七、实施计划（并行路径）

### 路径 1：模块 1（独立，最快完成）
1. 改 `App.tsx` + `QueryEditor.tsx`
2. 手动测试验收
3. 提交 PR 并合并

### 路径 2：模块 2 后端（P2a）
1. 改 `dby-core/ddl.rs`（4 函数 + 测试）
2. 改 `src-tauri/commands.rs`（4 命令）
3. 改 `src-tauri/lib.rs`（注册）
4. 改 `src/api.ts`（封装）
5. 跑 `cargo test/clippy/fmt`
6. 提交 PR 等待合并

### 路径 3：模块 2 前端（P2b，可与 P2a 部分并行）
1. 改 `SchemaTree.tsx` 类型和 UI（可立即开始）
2. 补 i18n 文案
3. 等 P2a 合并后接线测试
4. 跑 i18n check + 手动测试
5. 提交 PR 并合并

### 路径 4：模块 3（等模块 1 + 模块 2a 合并）
1. 改 `commands.rs`（`build_insert_sql`）
2. 改 `ResultsGrid.tsx`（菜单 UI）
3. 改 `App.tsx`（传 props）
4. 补 i18n 文案
5. 跑 CI + 手动测试
6. 提交 PR 并合并

**并行关系**：
- 路径 1 独立，先完成
- 路径 2、3 可并行（P2b 的 UI 改造不依赖后端，接线时同步）
- 路径 4 等路径 1、2 完成后开始

---

## 八、后续优化方向（本次不做）

1. **ResultsGrid 选中多行批量复制为 INSERT**：需前端多选状态管理。
2. **分类节点右键"新建"**：新建视图/函数/存储过程/触发器需要复杂编辑器，后续独立需求。
3. **列节点右键"查看引用"**：需要静态分析或元数据查询，架构级功能。
4. **连接标签页右键菜单**："关闭/关闭其他/断开连接"等，用户未要求，本次不做。
5. **历史面板右键菜单**：等价于现有图标按钮，本次不做。
6. **上下文菜单位置自适应**：接近屏幕边缘时自动调整方向，体验优化留后续。

---

## 九、文件清单总览

### 新增文件
无（全部是修改现有文件）

### 修改文件

**后端（Rust）**：
- `crates/dby-core/src/ddl.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`

**前端（TypeScript/React）**：
- `src/App.tsx`
- `src/api.ts`
- `src/components/QueryEditor.tsx`
- `src/components/SchemaTree.tsx`
- `src/components/ResultsGrid.tsx`

**国际化**：
- `src/locales/zh-CN.json`
- `src/locales/en-US.json`

**合计**：10 个文件。

---

## 十、估算工作量

- **模块 1**：0.5 人时（2 行代码 + 测试）
- **模块 2 后端（P2a）**：4 人时（4 函数 + 4 命令 + 测试）
- **模块 2 前端（P2b）**：6 人时（类型扩展 + 5 级节点改造 + 菜单项 + i18n）
- **模块 3**：4 人时（1 命令 + 前端菜单 UI + i18n）
- **总计**：14.5 人时

**并行执行预估**：
- 三个代理并行：约 6-8 人时（考虑合并等待时间）
- 串行执行：14.5 人时

**并行效率提升**：约 45%。
