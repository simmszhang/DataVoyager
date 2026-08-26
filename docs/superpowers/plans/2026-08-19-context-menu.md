# 右键菜单完善实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完善右键菜单系统：全局屏蔽浏览器原生菜单、Schema 树补齐各级节点菜单、修复视图删除 bug、ResultsGrid 新增右键菜单。

**Architecture:** 三模块并行架构：模块 1 全局屏蔽（2 行代码独立 PR）；模块 2 Schema 树分 P2a（后端 4 个 DDL 命令）+ P2b（前端菜单扩展）两子任务并行；模块 3 ResultsGrid（依赖模块 1）。后端命令复用现有 `run_ddl`/`guard_dangerous`/`parse_cells` 机制，SQL 生成走 Dialect 方言感知。

**Tech Stack:** Rust (dby-core + Tauri) + React 19 + TypeScript + i18next

**Spec:** `docs/design/specs/2026-08-19-context-menu-design.md`

## Global Constraints

- Rust: stable toolchain, MSVC on Windows
- 所有 SQL 生成必须走 `Dialect::quote_identifier`/`quote_string`（defects #4 已修复方向）
- 危险 SQL（DROP/TRUNCATE）必须接入 `guard_dangerous`（需 `confirmed: bool` 参数）
- i18n: 中英文文案必须成对添加到 `src/locales/zh-CN.json` 和 `en-US.json`，跑 `node src/locales/check-keys.mjs` 验证
- 提交前本地跑 CI 门禁：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p dby-core -p dby-driver-mysql && pnpm build`

---

## Task 1: 模块 1 - 全局右键菜单屏蔽

**Files:**
- Modify: `src/App.tsx:457` (添加 `onContextMenu`)
- Modify: `src/components/QueryEditor.tsx:70` (保留编辑器原生菜单)

**Interfaces:**
- Consumes: 无（独立改动）
- Produces: 全局屏蔽生效，自定义菜单不受影响

- [ ] **Step 1: App.tsx 添加全局屏蔽**

在 `src/App.tsx` 第 457 行最外层 `<div className="app">` 加 `onContextMenu` 处理器：

```tsx
return (
  <div className="app" onContextMenu={(e) => e.preventDefault()}>
    <header className="topbar">
```

- [ ] **Step 2: QueryEditor.tsx 保留编辑器原生菜单**

在 `src/components/QueryEditor.tsx` 第 70 行 `<div className="editor">` 加 `onContextMenu` 阻止冒泡：

```tsx
<div className="editor" onContextMenu={(e) => e.stopPropagation()}>
  <CodeMirror
```

- [ ] **Step 3: 手动测试验证**

启动开发服务器：
```bash
pnpm tauri dev
```

验证：
- 空白区域右键 → 无原生菜单
- Schema 树节点右键 → 弹自定义菜单（现有行为保持）
- CodeMirror 编辑器内右键 → 原生菜单保留（可复制/粘贴）

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx src/components/QueryEditor.tsx
git commit -m "feat(ui): 全局屏蔽浏览器原生右键菜单

- App.tsx: 最外层 div 阻止默认右键行为
- QueryEditor.tsx: 编辑器内保留原生菜单（stopPropagation）
- 为自定义右键菜单统一体验提供基础"
```

---

## Task 2: 模块 2a - 后端 DDL 函数（dby-core）

**Files:**
- Modify: `crates/dby-core/src/ddl.rs` (新增 4 个函数 + 单元测试)

**Interfaces:**
- Consumes: `Dialect::quote_identifier`
- Produces: 
  - `pub fn build_drop_view(dialect: &dyn Dialect, name: &str) -> String`
  - `pub fn build_drop_routine(dialect: &dyn Dialect, kind: &str, name: &str) -> String`
  - `pub fn build_drop_trigger(dialect: &dyn Dialect, name: &str) -> String`
  - `pub fn build_truncate_table(dialect: &dyn Dialect, name: &str) -> String`

- [ ] **Step 1: 写失败测试**

在 `crates/dby-core/src/ddl.rs` 末尾的 `#[cfg(test)] mod tests` 块内追加：

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

- [ ] **Step 2: 运行测试验证失败**

```bash
cargo test -p dby-core drop_view_and_routine
```

预期输出：`error[E0425]: cannot find function 'build_drop_view'` 等编译错误。

- [ ] **Step 3: 实现 4 个函数**

在 `crates/dby-core/src/ddl.rs` 的 `build_drop_table` 函数后追加（约 60 行后）：

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

- [ ] **Step 4: 运行测试验证通过**

```bash
cargo test -p dby-core drop_view_and_routine
```

预期输出：`test ddl::tests::drop_view_and_routine ... ok`

- [ ] **Step 5: 运行全量测试 + Clippy**

```bash
cargo test -p dby-core
cargo clippy -p dby-core -- -D warnings
cargo fmt --all --check
```

预期：全部通过，无警告。

- [ ] **Step 6: Commit**

```bash
git add crates/dby-core/src/ddl.rs
git commit -m "feat(core): 新增 drop_view/drop_routine/drop_trigger/truncate_table DDL 函数

- build_drop_view: 生成 DROP VIEW 语句
- build_drop_routine: 生成 DROP FUNCTION/PROCEDURE 语句
- build_drop_trigger: 生成 DROP TRIGGER 语句
- build_truncate_table: 生成 TRUNCATE TABLE 语句
- 单元测试验证 SQL 生成正确性（方言引号）"
```

---

## Task 3: 模块 2a - 后端 Tauri 命令

**Files:**
- Modify: `src-tauri/src/commands.rs` (新增 4 个命令)
- Modify: `src-tauri/src/lib.rs` (注册命令)
- Modify: `src/api.ts` (前端 API 封装)

**Interfaces:**
- Consumes: Task 2 的 `build_drop_view` 等函数、现有 `driver_for`/`run_ddl`/`guard_dangerous`
- Produces:
  - Tauri 命令：`drop_view(id: u64, database: String, name: String, confirmed: bool)`
  - Tauri 命令：`drop_routine(id: u64, database: String, kind: String, name: String, confirmed: bool)`
  - Tauri 命令：`drop_trigger(id: u64, database: String, name: String, confirmed: bool)`
  - Tauri 命令：`truncate_table(id: u64, database: String, name: String, confirmed: bool)`
  - 前端 API：`api.dropView(id, database, name, confirmed)`（同理其他三个）

- [ ] **Step 1: commands.rs 新增 4 个命令**

在 `src-tauri/src/commands.rs` 的 `drop_table` 函数后追加（约 1333 行后）：

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
    kind: String,
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

- [ ] **Step 2: lib.rs 注册命令**

在 `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏中追加四个命令（找到现有 `drop_table` 附近）：

```rust
.invoke_handler(tauri::generate_handler![
    // ...现有命令...
    drop_table,
    drop_view,
    drop_routine,
    drop_trigger,
    truncate_table,
    // ...其他命令...
])
```

- [ ] **Step 3: api.ts 封装前端 API**

在 `src/api.ts` 的 `export const api = { ... }` 对象内，`dropTable` 方法后追加：

```typescript
  dropView: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_view", { id, database, name, confirmed }),
  
  dropRoutine: (id: number, database: string, kind: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_routine", { id, database, kind, name, confirmed }),
  
  dropTrigger: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_trigger", { id, database, name, confirmed }),
  
  truncateTable: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("truncate_table", { id, database, name, confirmed }),
```

- [ ] **Step 4: 编译验证**

```bash
cargo build --manifest-path src-tauri/Cargo.toml
pnpm build
```

预期：编译通过，无错误。

- [ ] **Step 5: Clippy + fmt 检查**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt --all --check
```

预期：无警告，格式正确。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts
git commit -m "feat(tauri): 新增 drop_view/drop_routine/drop_trigger/truncate_table 命令

- commands.rs: 4 个命令复用 run_ddl + guard_dangerous 机制
- lib.rs: 注册 4 个命令到 Tauri invoke_handler
- api.ts: 前端 API 封装
- 接入现有危险 SQL 确认流程（confirmed 参数）"
```

---

## Task 4: 模块 2b - SchemaTree 类型扩展与菜单基础

**Files:**
- Modify: `src/components/SchemaTree.tsx:7-10` (扩展 MenuNode 类型)
- Modify: `src/components/SchemaTree.tsx:144-175` (新增 handler 函数)

**Interfaces:**
- Consumes: Task 3 的前端 API（`api.dropView` 等）
- Produces:
  - 扩展后的 `MenuNode` 类型（新增 category/view/routine/trigger/column 五种）
  - Handler 函数：`handleDropView`/`handleDropRoutine`/`handleDropTrigger`/`handleTruncateTable`/`copyToClipboard`

- [ ] **Step 1: 扩展 MenuNode 类型**

修改 `src/components/SchemaTree.tsx` 第 7-10 行：

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

- [ ] **Step 2: 新增 handler 函数**

在 `src/components/SchemaTree.tsx` 的 `handleDropTable` 函数后追加（约 175 行后）：

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

- [ ] **Step 3: 编译验证（暂时跳过 i18n 错误）**

```bash
pnpm build
```

预期：编译通过（TypeScript 类型检查通过），但运行时会缺 i18n 文案（Task 5 补充）。

- [ ] **Step 4: Commit**

```bash
git add src/components/SchemaTree.tsx
git commit -m "feat(tree): 扩展 MenuNode 类型 + 新增 5 个 handler 函数

- MenuNode 新增 category/view/routine/trigger/column 五种类型
- handleDropView/dropRoutine/dropTrigger/truncateTable: 调用后端 API
- copyToClipboard: 统一复制到剪贴板逻辑
- 准备接入各级节点右键菜单（i18n 文案待补充）"
```

---

## Task 5: 模块 2b - i18n 文案补充

**Files:**
- Modify: `src/locales/zh-CN.json` (新增 11 个键)
- Modify: `src/locales/en-US.json` (新增 11 个键)

**Interfaces:**
- Consumes: 无（独立文案）
- Produces: 11 个 i18n 键（`tree.menu.*` / `tree.*Confirm` / `tree.truncateSuccess`）

- [ ] **Step 1: zh-CN.json 补充中文文案**

在 `src/locales/zh-CN.json` 的 `"tree"` 对象内，`"menu"` 子对象追加（找到现有 `"dropTable"` 附近）：

```json
{
  "tree": {
    "menu": {
      "closeConnection": "关闭连接",
      "openConnection": "打开连接",
      "deleteConnection": "删除连接",
      "createTable": "创建表",
      "dropDatabase": "删除数据库",
      "queryData": "查询数据",
      "rename": "重命名",
      "dropTable": "删除表",
      "refresh": "刷新",
      "copyName": "复制名称",
      "truncateTable": "清空表",
      "dropView": "删除视图",
      "dropRoutine": "删除",
      "dropTrigger": "删除触发器",
      "copyColumnName": "复制列名",
      "copyColumnDef": "复制列定义"
    },
    "dropDatabaseConfirm": "确定要删除数据库 {{name}} 吗？",
    "dropTableConfirm": "确定要删除表 {{name}} 吗？",
    "dropViewConfirm": "确定要删除视图 {{name}} 吗？",
    "dropRoutineConfirm": "确定要删除 {{name}} 吗？",
    "dropTriggerConfirm": "确定要删除触发器 {{name}} 吗？",
    "truncateConfirm": "确定要清空表 {{name}} 的所有数据吗？此操作不可撤销！",
    "truncateSuccess": "表已清空",
    "renamePrompt": "请输入新表名",
    ...
  }
}
```

- [ ] **Step 2: en-US.json 补充英文文案**

在 `src/locales/en-US.json` 的 `"tree"` 对象内，`"menu"` 子对象追加：

```json
{
  "tree": {
    "menu": {
      "closeConnection": "Close Connection",
      "openConnection": "Open Connection",
      "deleteConnection": "Delete Connection",
      "createTable": "Create Table",
      "dropDatabase": "Drop Database",
      "queryData": "Query Data",
      "rename": "Rename",
      "dropTable": "Drop Table",
      "refresh": "Refresh",
      "copyName": "Copy Name",
      "truncateTable": "Truncate Table",
      "dropView": "Drop View",
      "dropRoutine": "Drop",
      "dropTrigger": "Drop Trigger",
      "copyColumnName": "Copy Column Name",
      "copyColumnDef": "Copy Column Definition"
    },
    "dropDatabaseConfirm": "Are you sure you want to drop database {{name}}?",
    "dropTableConfirm": "Are you sure you want to drop table {{name}}?",
    "dropViewConfirm": "Are you sure you want to drop view {{name}}?",
    "dropRoutineConfirm": "Are you sure you want to drop {{name}}?",
    "dropTriggerConfirm": "Are you sure you want to drop trigger {{name}}?",
    "truncateConfirm": "Are you sure you want to truncate table {{name}}? This will delete all data and cannot be undone!",
    "truncateSuccess": "Table truncated successfully",
    "renamePrompt": "Enter new table name",
    ...
  }
}
```

- [ ] **Step 3: 运行 i18n 检查**

```bash
node src/locales/check-keys.mjs
```

预期输出：`i18n check: PASS`（键对齐、无硬编码 CJK、t() 调用存在性全通过）。

- [ ] **Step 4: Commit**

```bash
git add src/locales/zh-CN.json src/locales/en-US.json
git commit -m "feat(i18n): 补充 Schema 树右键菜单文案（中英）

- tree.menu.*: 11 个新菜单项文案
- tree.*Confirm: 5 个确认对话框文案
- tree.truncateSuccess: 清空表成功提示
- 通过 i18n check-keys 验证"
```

---

## Task 6: 模块 2b - 各级节点 onContextMenu 接入

**Files:**
- Modify: `src/components/SchemaTree.tsx:236-343` (各级节点渲染处加 onContextMenu)

**Interfaces:**
- Consumes: Task 4 的 MenuNode 类型、Task 5 的 i18n 文案
- Produces: 7 级节点（分类/数据库/表/视图/函数存储过程/触发器/列）可弹右键菜单

- [ ] **Step 1: 分类节点加 onContextMenu**

找到 `src/components/SchemaTree.tsx` 约 236 行分类节点渲染处，在 `<div>` 上加：

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

- [ ] **Step 2: 表/视图节点区分 kind**

找到约 257-283 行表/视图渲染处，修改 `onContextMenu` 按 `cat` 区分：

```tsx
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

- [ ] **Step 3: 函数/存储过程节点加 onContextMenu**

找到约 315-326 行函数/存储过程渲染处：

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

- [ ] **Step 4: 触发器节点加 onContextMenu**

找到约 329-343 行触发器渲染处：

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

- [ ] **Step 5: 列节点加 onContextMenu**

找到约 286-304 行列渲染处：

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

- [ ] **Step 6: 数据库节点补充"复制库名"**

找到约 213-227 行数据库节点渲染处，确保有 `onContextMenu`（已存在，无需改动，只需在 Task 7 菜单项处理）。

- [ ] **Step 7: 编译验证**

```bash
pnpm build
```

预期：编译通过，TypeScript 类型检查通过。

- [ ] **Step 8: Commit**

```bash
git add src/components/SchemaTree.tsx
git commit -m "feat(tree): 7 级节点接入 onContextMenu

- 分类节点: 传 category 字段
- 表/视图节点: 按 cat 区分 kind (view/table)，修复视图删除 bug
- 函数/存储过程节点: 传 routineKind 字段
- 触发器节点: 传 trigger kind
- 列节点: 传 table + typeName 字段
- 准备菜单项构建逻辑（Task 7）"
```

---

## Task 7: 模块 2b - 菜单项构建逻辑

**Files:**
- Modify: `src/components/SchemaTree.tsx:352-386` (菜单项构建)

**Interfaces:**
- Consumes: Task 4 的 handler 函数、Task 6 的各级 MenuNode
- Produces: 完整的右键菜单项构建逻辑（8 种节点 × N 个菜单项）

- [ ] **Step 1: 扩展菜单项构建分支**

找到 `src/components/SchemaTree.tsx` 约 352 行 `const menuItems: ...` 处，替换为：

```tsx
const menuItems: { label: string; action: () => void }[] = [];
if (menu) {
  const node = menu.node;
  if (node.kind === "connection") {
    if (node.connId != null) {
      menuItems.push({
        label: t("tree.menu.closeConnection"),
        action: () => onDisconnect(node.connId as number),
      });
    } else {
      menuItems.push({
        label: t("tree.menu.openConnection"),
        action: () => onOpenSaved(node.configId),
      });
    }
    menuItems.push({
      label: t("tree.menu.deleteConnection"),
      action: () => onDeleteSaved(node.configId),
    });
  } else if (node.kind === "database") {
    menuItems.push({
      label: t("tree.menu.createTable"),
      action: () =>
        setCreateTable({ configId: node.configId, connId: node.connId, database: node.name }),
    });
    menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
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

- [ ] **Step 2: 手动测试所有菜单项**

启动开发服务器并连接 MySQL：
```bash
pnpm tauri dev
```

验证（需真实 MySQL 环境）：
- 分类节点右键 → 显示"刷新"，点击刷新列表
- 数据库节点右键 → 显示"创建表"、"复制库名"、"删除库"
- 表节点右键 → 显示"查询数据"、"复制名称"、"重命名"、"清空表"、"删除表"
- 视图节点右键 → 显示"查询数据"、"复制名称"、"删除视图"（点击后执行 `DROP VIEW`，不再报错）
- 函数/存储过程节点右键 → 显示"复制名称"、"删除"
- 触发器节点右键 → 显示"复制名称"、"删除触发器"
- 列节点右键 → 显示"复制列名"、"复制列定义"

- [ ] **Step 3: Commit**

```bash
git add src/components/SchemaTree.tsx
git commit -m "feat(tree): 完成 8 级节点右键菜单项构建

- category: 刷新
- database: 创建表、复制库名、删除库
- table: 查询数据、复制名称、重命名、清空表、删除表
- view: 查询数据、复制名称、删除视图（修复 bug）
- routine: 复制名称、删除
- trigger: 复制名称、删除触发器
- column: 复制列名、复制列定义
- 视图删除 bug 已修复（DROP TABLE → DROP VIEW）"
```

---

## Task 8: 模块 3 - 后端 build_insert_sql 命令

**Files:**
- Modify: `src-tauri/src/commands.rs` (新增命令)
- Modify: `src-tauri/src/lib.rs` (注册命令)
- Modify: `src/api.ts` (前端 API)

**Interfaces:**
- Consumes: 现有 `parse_cells`、`dby_core::edit::build_insert`
- Produces:
  - Tauri 命令：`build_insert_sql(id: u64, table: String, cells: Vec<(String, ColumnType, String)>)`
  - 前端 API：`api.buildInsertSql(id, table, cells)`

- [ ] **Step 1: commands.rs 新增 build_insert_sql 命令**

在 `src-tauri/src/commands.rs` 的 `build_edit_sql` 函数后追加（约 1134 行后）：

```rust
#[tauri::command]
pub async fn build_insert_sql(
    state: State<'_, Arc<AppState>>,
    id: u64,
    table: String,
    cells: Vec<(String, ColumnType, String)>,
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
    
    let parsed = parse_cells(&cells)?;
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

- [ ] **Step 2: lib.rs 注册命令**

在 `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏中追加：

```rust
.invoke_handler(tauri::generate_handler![
    // ...现有命令...
    build_edit_sql,
    build_insert_sql,
    execute_edit,
    // ...其他命令...
])
```

- [ ] **Step 3: api.ts 封装前端 API**

在 `src/api.ts` 的 `buildEditSql` 方法后追加：

```typescript
  buildInsertSql: (id: number, table: string, cells: EditCell[]) =>
    invoke<string>("build_insert_sql", { id, table, cells }),
```

- [ ] **Step 4: 编译验证**

```bash
cargo build --manifest-path src-tauri/Cargo.toml
pnpm build
```

预期：编译通过。

- [ ] **Step 5: Clippy 检查**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

预期：无警告。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts
git commit -m "feat(tauri): 新增 build_insert_sql 命令

- 复用 parse_cells 解析 EditCell
- 调用 dby_core::edit::build_insert 生成 INSERT 语句
- 走方言引号（Dialect::quote_identifier/quote_string）
- 为 ResultsGrid 复制为 INSERT 提供后端支持"
```

---

## Task 9: 模块 3 - ResultsGrid 右键菜单 UI

**Files:**
- Modify: `src/components/ResultsGrid.tsx:16-18,128` (Props + 状态)
- Modify: `src/components/ResultsGrid.tsx:214-225` (单元格 onContextMenu)
- Modify: `src/components/ResultsGrid.tsx:248-end` (菜单 UI)
- Modify: `src/App.tsx:587` (传 props)

**Interfaces:**
- Consumes: Task 8 的 `api.buildInsertSql`、现有 `onEditCell`/`displayCell`/`UNKNOWN_COLUMN_TYPE`
- Produces: ResultsGrid 右键菜单（4 个菜单项）

- [ ] **Step 1: ResultsGrid Props 新增字段**

在 `src/components/ResultsGrid.tsx` 第 16-18 行 `interface Props` 内追加：

```tsx
interface Props {
  result: StreamResult;
  onEditCell?: (rowIndex: number, colIndex: number, newValue: string) => void;
  tableName?: string | null;
  connId?: number | null;
}
```

同时修改第 125 行 `export default function ResultsGrid` 签名：

```tsx
export default function ResultsGrid({ result, onEditCell, tableName, connId }: Props) {
```

- [ ] **Step 2: 新增菜单状态**

在 `src/components/ResultsGrid.tsx` 第 128 行 `const [editing, ...]` 后追加：

```tsx
const [menu, setMenu] = useState<{ x: number; y: number; row: number; col: number } | null>(null);
```

- [ ] **Step 3: 单元格加 onContextMenu**

找到约 214-225 行单元格渲染处，在 `<div className="grid-cell">` 上加：

```tsx
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
  onContextMenu={(e) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, row: vi.index, col: j });
  }}
>
```

- [ ] **Step 4: 菜单项逻辑与渲染**

在 `src/components/ResultsGrid.tsx` 的 `return` 块内，`</div>` 闭合标签前追加：

```tsx
{menu && result.columns && (
  (() => {
    const row = result.rows[menu.row];
    const cell = row[menu.col];
    const columns = result.columns;
    
    const menuItems: { label: string; action: () => void }[] = [];
    
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
            // 失败静默
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
          onEditCell(menu.row, menu.col, "NULL");
          setMenu(null);
        },
      });
    }
    
    return (
      <>
        <div className="ctx-overlay" onClick={() => setMenu(null)} />
        <div className="ctx-menu" style={{ left: menu.x, top: menu.y }}>
          {menuItems.map((m, i) => (
            <div key={i} className="ctx-item" onClick={() => m.action()}>
              {m.label}
            </div>
          ))}
        </div>
      </>
    );
  })()
)}
```

- [ ] **Step 5: App.tsx 传入新 Props**

找到 `src/App.tsx` 约 587 行 `<ResultsGrid>` 调用处：

```tsx
<ResultsGrid
  result={ws.result}
  onEditCell={handleEditCell}
  tableName={ws.selectedTable}
  connId={activeId}
/>
```

- [ ] **Step 6: 补充 i18n 文案**

在 `src/locales/zh-CN.json` 的 `"grid"` 对象内追加：

```json
{
  "grid": {
    "null": "NULL",
    "executedAffected": "执行成功，影响 {{count}} 行",
    "lastInsertId": "，最后插入 ID: {{id}}",
    "rowsReturned": "返回 {{count}} 行",
    "truncated": "（已截断）",
    "truncatedTitle": "结果集超过行数上限，已截断",
    "truncatedSuffix": "（结果已截断）",
    "doubleClickEdit": "（双击单元格编辑）",
    "menu": {
      "copyCell": "复制单元格",
      "copyRowAsJson": "复制行为 JSON",
      "copyAsInsert": "复制为 INSERT 语句",
      "setNull": "设为 NULL"
    }
  }
}
```

在 `src/locales/en-US.json` 的 `"grid"` 对象内追加：

```json
{
  "grid": {
    "null": "NULL",
    "executedAffected": "Executed successfully, {{count}} row(s) affected",
    "lastInsertId": ", last insert ID: {{id}}",
    "rowsReturned": "{{count}} row(s) returned",
    "truncated": "(truncated)",
    "truncatedTitle": "Result set exceeds row limit and has been truncated",
    "truncatedSuffix": "(result truncated)",
    "doubleClickEdit": "(Double-click cell to edit)",
    "menu": {
      "copyCell": "Copy Cell",
      "copyRowAsJson": "Copy Row as JSON",
      "copyAsInsert": "Copy as INSERT",
      "setNull": "Set to NULL"
    }
  }
}
```

- [ ] **Step 7: 运行 i18n 检查**

```bash
node src/locales/check-keys.mjs
```

预期输出：`i18n check: PASS`

- [ ] **Step 8: 编译验证**

```bash
pnpm build
```

预期：编译通过。

- [ ] **Step 9: 手动测试所有菜单项**

启动开发服务器：
```bash
pnpm tauri dev
```

验证：
- 单元格右键 → 弹菜单
- "复制单元格" → 剪贴板有单元格值
- "复制行为 JSON" → 剪贴板有 JSON 对象
- "复制为 INSERT" → 剪贴板有 INSERT 语句（表名存在时）、无表名时该项不显示
- "设为 NULL" → 单元格变 NULL（可编辑时显示）

- [ ] **Step 10: Commit**

```bash
git add src/components/ResultsGrid.tsx src/App.tsx src/locales/zh-CN.json src/locales/en-US.json
git commit -m "feat(grid): 新增 ResultsGrid 右键菜单

- Props 新增 tableName/connId 字段
- 4 个菜单项：复制单元格/复制行 JSON/复制为 INSERT/设为 NULL
- 复制为 INSERT 走后端 build_insert_sql（方言引号）
- 设为 NULL 复用现有 onEditCell('NULL') 机制
- i18n 文案补充（中英）"
```

---

## Task 10: 最终集成测试与文档更新

**Files:**
- Modify: `docs/design/requirements.md` (更新 R12 状态)
- Create: `docs/design/context-menu-testing.md` (测试报告，可选)

**Interfaces:**
- Consumes: 所有前序任务
- Produces: 完整功能验证 + 文档更新

- [ ] **Step 1: 运行完整 CI 门禁**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dby-core -p dby-driver-mysql
pnpm build
node src/locales/check-keys.mjs
```

预期：全部通过。

- [ ] **Step 2: 完整手动测试清单**

启动应用并连接真实 MySQL：
```bash
pnpm tauri dev
```

**模块 1 验证**：
- [ ] 空白区域右键 → 无原生菜单
- [ ] CodeMirror 编辑器右键 → 原生菜单保留

**模块 2 验证（Schema 树）**：
- [ ] 分类节点（Tables/Views/Functions/Procedures/Triggers）右键 → 显示"刷新"，点击刷新列表
- [ ] 数据库节点右键 → "创建表"、"复制库名"、"删除库"全可用
- [ ] 表节点右键 → 5 个菜单项全可用，"清空表"成功清空数据
- [ ] 视图节点右键 → "删除视图"成功执行 `DROP VIEW`（不再报错，bug 已修复）
- [ ] 函数节点右键 → "删除"成功执行 `DROP FUNCTION`
- [ ] 存储过程节点右键 → "删除"成功执行 `DROP PROCEDURE`
- [ ] 触发器节点右键 → "删除触发器"成功执行 `DROP TRIGGER`
- [ ] 列节点右键 → "复制列名"和"复制列定义"成功写入剪贴板

**模块 3 验证（ResultsGrid）**：
- [ ] 查询有表名的结果集，单元格右键 → 4 个菜单项全显示
- [ ] "复制单元格" → 剪贴板有单元格值
- [ ] "复制行为 JSON" → 剪贴板有格式化 JSON
- [ ] "复制为 INSERT" → 剪贴板有 `INSERT INTO` 语句，SQL 引号正确
- [ ] "设为 NULL" → 单元格变 NULL，刷新查询后仍为 NULL
- [ ] 无表名时（如 `SELECT 1`）→ "复制为 INSERT" 不显示

- [ ] **Step 3: 更新 requirements.md**

编辑 `docs/design/requirements.md`，将 R12 状态改为：

```markdown
| R12 | 右键菜单完善：(1) 全局屏蔽浏览器原生右键菜单；(2) Schema 树补齐各级节点右键菜单（分类/视图/函数/存储过程/触发器/列）+ 修复视图删除 bug（DROP TABLE → DROP VIEW）；(3) ResultsGrid 新增右键菜单（复制单元格/行/INSERT、设 NULL） | P1 | 大 | 已实现 | specs/2026-08-19-context-menu-design.md；新增 5 个后端命令（drop_view/drop_routine/drop_trigger/truncate_table/build_insert_sql）；涉及 10 个文件（dby-core/ddl + commands + SchemaTree + ResultsGrid + i18n）；通过 CI 门禁 + 手动测试验证 |
```

- [ ] **Step 4: Commit 文档更新**

```bash
git add docs/design/requirements.md
git commit -m "docs: 更新 R12 需求状态为已实现

- 右键菜单完善功能全部完成
- 通过 CI 门禁（fmt/clippy/test/build/i18n-check）
- 通过完整手动测试验证
- 10 个文件改动，5 个新增后端命令"
```

- [ ] **Step 5: 最终确认**

检查：
- [ ] 所有 10 个任务的 commit 都已提交
- [ ] `git log --oneline` 看到 10 条 commit（模块 1 × 1 + 模块 2a × 2 + 模块 2b × 4 + 模块 3 × 2 + 集成 × 1）
- [ ] 分支可合并到 `master`（无冲突）
- [ ] CI 门禁全绿

完成！右键菜单完善功能已全部实现并验证。

---

## 并行执行建议

按设计文档的三模块并行架构，可以这样分配：

**代理 A（模块 1 + 集成）**：
- Task 1（全局屏蔽）→ 立即合并
- Task 10（最终集成测试）→ 等其他任务完成后执行

**代理 B（模块 2a 后端）**：
- Task 2（dby-core DDL 函数）
- Task 3（Tauri 命令）
- 完成后通知代理 C 可接线

**代理 C（模块 2b 前端）**：
- Task 4（MenuNode 类型 + handler）→ 可立即开始
- Task 5（i18n 文案）→ 可立即开始
- Task 6（onContextMenu 接入）→ 等 Task 4 完成
- Task 7（菜单项构建）→ 等 Task 3、6 完成后接线测试

**代理 D（模块 3）**：
- 等 Task 1 合并后开始
- Task 8（build_insert_sql 命令）
- Task 9（ResultsGrid 菜单 UI）

**依赖关系**：
- Task 6、7 依赖 Task 3（后端 API）
- Task 9 依赖 Task 1（全局屏蔽）、Task 8（build_insert_sql）
- Task 10 依赖所有前序任务

**预计时间线**（并行执行）：
- 第 1 轮（0-2h）：A 完成 Task 1，B 开始 Task 2-3，C 开始 Task 4-5
- 第 2 轮（2-4h）：B 完成 Task 2-3，C 完成 Task 4-6，D 开始 Task 8
- 第 3 轮（4-6h）：C 完成 Task 7，D 完成 Task 8-9
- 第 4 轮（6-7h）：A 执行 Task 10 集成测试

**总耗时约 7 小时**（相比串行 14.5 小时节省 52%）。
