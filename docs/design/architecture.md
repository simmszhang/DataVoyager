# 总体架构与核心抽象

## 1. 分层架构（Cargo workspace）

```
dby/
├── crates/
│   ├── dby-core/          纯 Rust 引擎（零 Tauri 依赖，可 cargo test）
│   └── dby-driver-mysql/  MySQL 驱动（impl core traits）
├── src-tauri/             Tauri 桌面壳（薄层：commands / state / setup）
└── src/                   React 前端（Zustand 状态 + 组件）
```

- **引擎/驱动/壳三层解耦**：`dby-core` 无 GUI 依赖，驱动可被 CLI/MCP 复用。
- **边界问题**：引擎侧目前是「驱动 trait + 方言 + 类型映射 + DDL/edit/danger/export 生成器」，但 SQL 生成（`edit.rs`/`ddl.rs`）落在 `dby-core` 而执行在壳层 `commands.rs`，职责边界基本清晰，但「方言感知」没有贯穿到前端（见 defects #4）。

## 2. 统一 `Value` 类型

带 tag 的 JSON envelope（`{"t": "...", "v": ...}`），跨进程无损表示单元格：

- 覆盖 Null / Bool / I64 / U64 / F64 / Decimal / Str / Bytes / Date / Time / DateTime / Json / Uuid / Array / Map。
- 前端据此着色、格式化、选编辑控件。

**已知缺口**：MySQL 文本协议下 DECIMAL 与 Date/DateTime 的类型判定是启发式的（见 defects #1）。

## 3. `Driver` / `Connection` 抽象

- `Driver`：`id` / `display_name` / `capabilities`（能力矩阵）/ `dialect` / `connect`。
- `Connection`（async trait）：`ping` / `catalogs` / `schemas` / `tables` / `columns` / `indexes` / `foreign_keys` / `triggers` / `procedures` / `table_ddl` / `execute_stream` / `begin` / `commit` / `rollback` / `set_autocommit`。
- 流式执行：`execute_stream(..., sink: &mut dyn ResultSink)` 把 `StreamEvent`（Columns/Rows/Affected/Info）逐批推给 sink；缓冲路径 `execute_buffered` 用 `CollectingSink` 收拢。
- 取消：`ExecOpts.cancel: Option<CancellationToken>`，驱动**每批之间**检查（drain 式，见 defects #5）。

## 4. SQL 方言（`Dialect`）

`quote_identifier` / `quote_string` / `limit_clause` / `display_type_name`，供 SQL 生成（edit/ddl/export）与语句切分（`split_statements`）使用。

**缺口**：前端查询生成（`SELECT * FROM \`表\``）没有走方言（见 defects #4）。

## 5. 持久化（三类数据三种存储）

| 数据 | 存储 | 位置 |
| --- | --- | --- |
| 项目/连接配置 | JSON（`dby-core::config`） | 应用数据目录 `config.json` |
| 密码/SSH 凭据 | OS 钥匙串（`keyring`） | 系统凭据库 |
| SQL 历史 | SQLite + FTS5（`dby-core::history`） | 应用数据目录 `history.db` |

**缺口**：历史写入是同步的（见 defects #13）；语句库删除未实现（见 defects #14）。

## 6. IPC 数据流

- 编辑器「运行」→ `execute_query_stream`（Tauri `Channel` 流式推 `StreamEvent`）→ 前端增量 append + 虚拟滚动。
- 元数据/编辑/导出/DDL → 缓冲路径（`execute_buffered`）→ 返回 `QueryOutput`。
- 所有执行经引擎 `execute*` 统一归因到历史（origin：manual_editor/data_edit/schema_edit/export）。

## 7. 前端状态（Zustand）

`workspaces: Record<connId, WorkspaceState>` 每连接独立工作态（query/result/事务/选中库表），`tabs` 管理打开的连接标签，`mutateResult` 就地追加流式结果 + 版本号触发渲染。

**缺口**：workspace 内残留 `databases/tables/columns` 字段（现由 SchemaTree 自管，见 defects #15）。
