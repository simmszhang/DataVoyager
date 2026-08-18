# #4 方言感知 SQL 生成 — 设计文档

> 状态：评审需修订（2 阻断项已修订，待复审） · 优先级 P1 · 规模：大 · 关联缺陷：#3（节点 key）、#59（split 并入 Dialect）、#61（元数据拼接）· 依赖共享契约：S1（并发范式）、S5（错误形状）

## 1. 现状与影响

- `App.tsx:130-136`：`handleOpenTable` 生成 `` `SELECT * FROM \`${table}\` LIMIT 100;` `` 硬编码 MySQL 反引号（#4）——**这是前端仅剩的 SQL 生成点**。
- `SchemaTree.tsx`：树节点 key 用 `c:5`/`d:5:库名`/`t:5:库名:表名` 靠 `:` 拼接再 split 还原（#3）。
- `dialect.rs:18-114`：`split_statements` 是方言无关自由函数，不支持 Postgres dollar-quoting（#59，review C9）。
- `lib.rs:154-159,235-239`：仅 `indexes`（`SHOW INDEX FROM {q} FROM {q}`）、`table_ddl`（`SHOW CREATE TABLE {q}.{q}`）用 `format!` 拼标识符（#61，review D6）；`columns/foreign_keys/tables` 已走参数化 `exec(sql, params)`。
- **影响**：接 PostgreSQL/SQLite 时表浏览查询生成出错，违背「方言感知」（architecture §4）；节点 key 脆弱；语句切分随多方言失效。

## 2. 目标与成功标准

1. 前端不再生成任何 SQL，表浏览 SQL 由 `dby-core` 的 `Dialect` 生成。
2. 树节点 key 改结构化编码，不再 `:` 拼接 + split。
3. `split_statements` 并入 `Dialect`（默认实现 + 未来驱动覆盖）。
4. 元数据 SQL 的 `format!` 例外显式标注（#61）。
5. 成功标准：全前端 grep 无 MySQL 反引号/`LIMIT` 硬编码；节点 key 稳定可扩展。

## 3. 方案对比

### 方案 A：SQL 生成收口 `dby-core` + 新命令（推荐）
- 新增 `build_table_select(dialect, table, limit)` 落在 `dby-core`；壳层加命令；前端调用拿 SQL。
- **优点**：符合 architecture §1；接新驱动零前端改动。**缺点**：需新增命令。

### 方案 B：前端引入 dialect 描述对象自行拼
- **缺点**：SQL 生成仍在前端，违背「前端只传结构化参数」，否决。

### 方案 C：仅把反引号换成前端传入 quote 字符
- **缺点**：仍是前端拼 SQL，否决。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 `build_table_select`（`crates/dby-core/src/query.rs` 或新 `browse.rs`）

```rust
pub fn build_table_select(dialect: &dyn Dialect, table: &str, limit: Option<u64>) -> String {
    let q = dialect.quote_identifier(table);
    match limit {
        Some(n) => format!("SELECT * FROM {q} {}", dialect.limit_clause(Some(n), None)),
        None => format!("SELECT * FROM {q}"),
    }
}
```

> **schema 处理**：`SELECT * FROM {table}` 不带库名前缀；当前库上下文由驱动 `execute_stream` 的 `USE {db}`（`lib.rs:254-261`）负责，前端 `handleOpenTable` 仍设置 `selectedDb` 即可。此与「方言感知」不冲突（`USE` 是连接态行为，非 SQL 生成）。

壳层命令（`commands.rs`，遵循 S1 的 `futures::lock::Mutex` 持锁范式）：

```rust
#[tauri::command]
pub async fn build_table_select(state: State<'_, Arc<AppState>>, id: u64, table: String) -> Result<String> {
    let entry = state.connections.lock().unwrap().get(&id).cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let active = entry.lock().await; // 只读 driver_id
    let driver = state.registry.resolve(&active.driver_id)?;
    Ok(dby_core::query::build_table_select(driver.dialect(), &table, Some(100)))
}
```

前端 `handleOpenTable`（`App.tsx:130-136`）改为 `async`，调用 `api.buildTableSelect(connId, table)` 填入 `query`。

### 4.2 节点 key 结构化（#3，`src/components/SchemaTree.tsx`）

```ts
type NodeKey =
  | { kind: "conn"; connId: number }
  | { kind: "db"; connId: number; db: string }
  | { kind: "table"; connId: number; db: string; table: string }
  | { kind: "column"; connId: number; db: string; table: string; column: string };
const keyOf = (k: NodeKey) => JSON.stringify(k);
const parseKey = (key: string): NodeKey => JSON.parse(key);
```

### 4.3 `split_statements` 并入 `Dialect`（#59）

```rust
pub trait Dialect: Send + Sync {
    // ...既有方法
    fn split_statements<'a>(&self, sql: &'a str) -> Vec<&'a str> {
        generic_split_statements(sql) // 默认实现 = 现有逻辑（单/双引号、反引号、--/#、/* */）
    }
}
```

- 现有自由函数 `split_statements` 保留为**薄封装**（委托 `generic_split_statements`，与 trait 默认方法同源），供 `danger.rs` 等既有调用点继续使用。
- **不改变 `analyze_danger` 的签名**：danger 分析仍用方言无关切分（现状）；`Dialect::split_statements` 的能力供未来 Postgres 驱动覆盖 dollar-quoting 时使用，届时再让 danger 分析走方言切分（与 #48 的 tokenizer 升级协调，本方案不越界改 #48 的 `analyze_danger`）。

### 4.4 元数据 SQL 参数化标注（#61）

- 已参数化的（`columns`/`foreign_keys`/`tables` 的 `exec(sql, params)`）保持不变。
- 仅 `indexes`（`SHOW INDEX FROM {q} FROM {q}`）、`table_ddl`（`SHOW CREATE TABLE {q}.{q}`）用 `format!` 拼标识符——两者均属「标识符不可参数化」的 MySQL 语法限制，保留 `quote_identifier` 转义并加注释 `// 非参数化例外：SHOW INDEX/SHOW CREATE TABLE 不支持占位符；标识符已 quote_identifier 转义`。

## 5. 错误处理（遵循 S5）

- `build_table_select` 连接不存在：`DbError::ConnectionNotFound`（kind 化后前端可区分）。

## 6. 测试策略

- **单元（dby-core）**：`build_table_select` 用 MySQL dialect 产出 `` SELECT * FROM `t` LIMIT 100 ``；用测试方言（双引号）产出 `SELECT * FROM "t" LIMIT 100`。
- **单元（dialect）**：`split_statements` 默认实现回归既有用例；dollar-quoting 由未来 Postgres 覆盖。
- **前端（手工）**：`handleOpenTable` 不再拼 SQL；节点 key 往返稳定。

## 7. 回归风险与影响面

- `handleOpenTable` 从同步拼串改为 async 调命令（需 `await` + `try/catch`）。
- 壳层命令遵循 S1 持锁范式（`futures::lock::Mutex`）。
- 节点 key 变更：`SchemaTree` 展开/选择回调解 key 逻辑同步。
- `split_statements` 改 trait 默认方法：`danger.rs` 调用点经薄封装无感；未来驱动可覆盖。

## 8. 关联缺陷处置

- #4：4.1；#3：4.2；#59：4.3；#61：4.4。

## 9. 与其它方案组的依赖

- 与 #1 共享 `Dialect` trait 扩展（各自新增方法，无冲突）；与 #48 的边界：本方案只加 `Dialect::split_statements`，不改 `analyze_danger`（#48 负责 tokenizer）；依赖 S1（`build_table_select` 命令的持锁范式）、S5。
