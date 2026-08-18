# 总体架构与核心抽象

> 本文记录「应然」设计（含评审后新增的横切关注点）。实现与设计的差距见 `defects.md`（缺陷清单）与 `review.md`（逐层审查证据，引用如 C1/D3/S1/F2）。

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
- **职责边界**：SQL 生成（`edit.rs`/`ddl.rs`/`export.rs`/`danger.rs`）落在 `dby-core`，执行在壳层 `commands.rs`。「方言感知」尚未贯穿到前端（defects #4，review F 层多处硬编码反引号）。

## 2. 统一 `Value` 类型

带 tag 的 JSON envelope（`{"t": "...", "v": ...}`），跨进程无损表示单元格：

- 覆盖 Null / Bool / I64 / U64 / F64 / Decimal / Str / Bytes / Date / Time / DateTime / Json / Uuid / Array / Map（`value.rs`）。
- 前端据此着色、格式化、选编辑控件。

**评审更新**：当前驱动（`conv.rs`）**不产出** `Decimal/Bool/Json/Uuid/Array/Map` 六个变体——DECIMAL 落 `Str`、JSON 列落 `Str`、BLOB/BINARY 偶发被 UTF-8 误判 `Str`（review D1）。精确类型判定应收口到「类型映射层」（§8）。另：`I64/U64` 以 JSON number 跨 IPC，>2^53 丢精度（review S11）。

## 3. `Driver` / `Connection` 抽象

- `Driver`：`id` / `display_name` / `capabilities`（能力矩阵）/ `dialect` / `connect`。
- `Connection`（async trait）：`ping` / `catalogs` / `schemas` / `tables` / `columns` / `indexes` / `foreign_keys` / `triggers` / `procedures` / `table_ddl` / `execute_stream` / `begin` / `commit` / `rollback` / `set_autocommit`。
- 流式执行：`execute_stream(..., sink: &mut dyn ResultSink)` 把 `StreamEvent`（Columns/Rows/Affected/Info）逐批推给 sink；缓冲路径 `execute_buffered` 用 `CollectingSink` 收拢。
- 取消：`ExecOpts.cancel: Option<CancellationToken>`，驱动**每批之间**检查（drain 式，defects #5）。评审发现取消实际因全局锁 + 令牌不 reset 而双重失效（review S1/S3）。

**评审更新**：`Connection` 声明 15 方法，但壳层只暴露 `schemas/tables/columns`，`ping/indexes/foreign_keys/triggers/procedures/table_ddl/catalogs` 无对应 command（review S13），能力矩阵在前端「落空」（review F5）。

## 4. SQL 方言（`Dialect`）

`quote_identifier` / `quote_string` / `limit_clause` / `display_type_name`，供 SQL 生成（edit/ddl/export）使用。

**缺口**：前端查询生成（`SELECT * FROM \`表\``）没有走方言（defects #4）；`split_statements` 目前是方言无关的自由函数（非 `Dialect` 方法），不支持 Postgres dollar-quoting（review C9），随 M2 驱动接入时应并入 `Dialect`。

## 5. 持久化（三类数据三种存储）

| 数据 | 存储 | 位置 |
| --- | --- | --- |
| 项目/连接配置 | JSON（`dby-core::config`） | 应用数据目录 `config.json` |
| 密码/SSH 凭据 | OS 钥匙串（`keyring`） | 系统凭据库 |
| SQL 历史 | SQLite + FTS5（`dby-core::history`） | 应用数据目录 `history.db` |

**评审更新**：`架构承诺`与实际不符——只有 MySQL 密码进钥匙串，**SSH 密码/私钥明文落 `config.json` 并经 IPC 返回前端**（review S2，🔴）。历史写入同步（defects #13）、语句库删除未实现（defects #14）、`clear` 不清理 FTS + 跨项目归因有损（review C4）、config 非原子写（review C8）。

## 6. IPC 数据流

- 编辑器「运行」→ `execute_query_stream`（Tauri `Channel` 流式推 `StreamEvent`）→ 前端增量 append + 虚拟滚动。
- 元数据/编辑/DDL → 缓冲路径（`execute_buffered`）→ 返回 `QueryOutput`。
- 导出 → `export_result`（缓冲 + 格式化，返回字符串）。
- 所有执行经引擎 `execute*` 统一归因到历史（origin 共 8 种：manual_editor/data_edit/schema_edit/export/ai/plugin/cli/other）。

**评审更新**：流式无终止事件（`StreamEvent` 无 `Done`/`Error`），channel 事件与 `invoke` 返回是两条传输，收尾有竞态（review S8）；`truncated`/`info` 未接线（review F3）；导出全量收集进内存、整串过 IPC（review S4）。完整 36 命令契约见 §13 与 `review.md` §六。

## 7. 前端状态（Zustand）

`workspaces: Record<connId, WorkspaceState>` 每连接独立工作态（query/result/事务/选中库表），`tabs` 管理打开的连接标签，`mutateResult` 就地追加流式结果。

**评审更新**：`resultVersion` 是**只写不读**的死字段，实际靠返回新 `workspaces` 引用触发渲染（review F11）；workspace 残留 `databases/tables/columns` 死字段（defects #15）；`mutateResult` 在 `set` 内就地 mutate 破坏不可变约定（review F12）。

---

## 8. 类型映射层（评审新增 · 支撑 #1/#11/#20 修复）

- **目标**：以**列类型驱动**的值转换，替代当前驱动内启发式（`conv.rs`）。
- **组成**：
  1. 结构化列类型 `ColumnType`（基类型 + `numeric_precision/scale`、`unsigned`、`char_max_length`、`charset/collation`、`ordinal_position`），扩展 `ColumnInfo`（当前只有 `type_name` 字符串，review C5）。
  2. `Dialect` 提供「原生类型名 → `ColumnType`」解析（消除当前两条路径不一致，review D3；替代 `format!("{:?}")` 的脆弱实现，defects #20）。
  3. 映射规则：BLOB/BINARY→`Bytes`、JSON→`Json`、DECIMAL→`Decimal`（保留原串）、`TINYINT(1)`→可选 `Bool`、Date/Time/DateTime 按列类型而非时分秒推断（fixes #1）。
- **边界**：值编辑（`build_edit_sql`）的「前端输入 → 正确 `Value`」也走此层（fixes #11、review F16）。

## 9. 错误模型（评审新增 · 支撑 #19 修复）

- `DbError` 8 变体：`Database/DriverNotFound/ConnectionNotFound/Unsupported/Config/Storage/Cancelled/Other`（`error.rs`）。
- **IPC 契约**：当前只序列化 `{"message": "…"}`，**丢失 variant 种类**（review C2）——前端无法程序化区分「取消」与「失败」。应序列化为 `{"kind": "...", "message": "..."}`。
- 前端统一 `ApiError` 类型 + 全局 toast/横幅恢复（defects #19）；`history.record`/`set_secret` 错误不得静默吞（review S5）。

## 10. 多结果集语义（评审新增）

- `StreamEvent` 需新增 `ResultSetEnd`（或 `Columns` 带结果集序号）；`CollectingSink` 按边界分桶，`QueryOutput.result_sets` 真正填多组（当前永远只填一个，review C1）。
- 驱动 `query_iter` 需 `next_result_set()` 遍历全部结果集（当前只读首个，其余静默丢弃，review D4）。
- 支撑：存储过程 `CALL`、多语句、未来 Postgres 多结果集。

## 11. 并发模型（评审新增 · 支撑 S1/S3 修复）

- **现状**：`connections: Mutex<HashMap<u64, ActiveConnection>>` 单把全局锁，所有命令在锁内 `await` 整个网络 I/O（review S1，🔴）——任意慢查询串行化全部连接，`cancel_query` 抢不到锁而失效。
- **目标**：连接注册表（`HashMap`/`DashMap`）+ **per-connection 锁**；命令按 id 取到 `Arc<Mutex<ActiveConnection>>` 后只锁单连接。
- **取消**：`CancellationToken` 按**查询实例**创建（每次 `execute_*` 新建 token，登记「查询 id→token」），`cancel_query` 只读 token 的 `Arc` 无需抢连接锁；当前每连接单令牌且永不 reset（review S3，🔴）。

## 12. 安全模型（评审新增）

- **凭据**：MySQL 密码进 keyring（已做）；SSH 密码/私钥**必须**同样进 keyring，`SshOptions.password/private_key` 加 `skip_serializing`，`list_saved_connections` 返回脱敏视图（fixes S2）。
- **SSH**：TOFU 主机指纹确认（fixes #9）；私钥认证 `russh-keys`（fixes #8）；隧道生命周期可取消、`Drop` 释放（fixes D7）；连接/转发超时 + 错误可读（fixes D8）。
- **SSL**：`verify_cert=false` 接受任意证书；CA 校验/双向证书未实现（M2）。
- **壳层安全**：启用 CSP（当前 `csp: null`）、破坏性命令服务端二次确认/ACL（review S15）。

## 13. IPC 契约（评审新增）

共 **36 个 Tauri command**（9 组：驱动/连接、元数据、执行、事务、数据编辑、DDL、导出、项目、历史）。完整参数/返回形状见 `review.md` §六（含 4 处契约漂移）：

1. `list_saved_connections` 泄 ssh.password（S2）；
2. `id`/`last_insert_id`/`I64/U64` 以 JSON number 跨 IPC，>2^53 丢精度（S11）；
3. `analyze_danger` 的 `warn` 变体后端从不产出（C7）；
4. 流式无终止事件（S8）。

**错误形状**：`{"message": string}`（应升级为带 `kind`，见 §9）。
