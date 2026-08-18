# 设计评审记录（代码逐层审查）

> 本文件记录「对照代码的逐层审查」结论：设计文档与代码的差异、以及 `defects.md` 尚未覆盖的新设计缺口。
> 它是 `defects.md` 的**来源与补充**——评审确认后的新缺口应并入 `defects.md` 统一跟踪，本文件保留审查证据（含 file:line）。

## 审查范围与方法

四层逐一读源码，与 `architecture.md` / `subsystems.md` / `defects.md` 对照：

| 层 | 范围 | 状态 |
| --- | --- | --- |
| 引擎 `dby-core` | 13 个源文件 | ✅ |
| 驱动 `dby-driver-mysql` | lib/conv/dialect/tunnel + 集成测试 | ✅ |
| 壳层 `src-tauri` | commands/state/lib + capabilities/tauri.conf | ✅ |
| 前端 `src` | store/api/App + 全部组件 | ✅ |

---

## 一、引擎 `dby-core`

### 1.1 文档↔代码差异

- `architecture.md` 与 `AGENTS.md` 只列了 4 个历史归因来源，但代码 `SqlOrigin` 实为 **8 个变体**，已前瞻性建模 `Ai`/`Plugin`/`Cli`/`Other`（`query.rs:24-34`）。
- `subsystems.md §9` 称导出「受 2000 行 `max_rows` 截断」——**与代码不符**：壳层所有执行都用 `ExecOpts::default()`（`max_rows=None` → `usize::MAX`），导出**不截断、全量收集进内存**（见 §三 S4）。

### 1.2 新增设计缺口

#### 🟡 C1 多结果集无边界事件，缓冲路径会错误合并
- **现状**：`StreamEvent` 只有 `Columns/Rows/Affected/Info`，无「结果集结束」标记（`query.rs:91-96`）；`CollectingSink` 对每个 `Columns` 仅**覆盖** `self.columns`，所有 `Rows` 追加到**同一** `rows`（`query.rs:162-183`）。`QueryOutput.result_sets` 建模为 `Vec`，但 `CollectingSink::into_output` 永远只产出**一个** `ResultSet`（`query.rs:143-158`）。
- **影响**：一条返回多个结果集的 SQL（存储过程、多语句）在缓冲路径被错误合并；驱动侧 `query_iter` 也**只读首个结果集**（`driver lib.rs:263-300`，未 `next_result_set()`），其余被静默丢弃。
- **方向**：新增 `ResultSetEnd` 流事件（或 `Columns` 带结果集序号）；`CollectingSink` 按边界分桶；驱动遍历所有结果集。属架构级，建议与 #1 类型映射一起定方案。

#### 🟡 C2 `DbError` 跨 IPC 丢失错误种类
- **现状**：`DbError` 的 `Serialize` 只输出 `{"message": "…"}`（`error.rs:28-38`），variant（`Cancelled`/`Database`/`Storage`/`Config` 等）不传给前端。
- **影响**：前端无法程序化区分「已取消」与「真失败」，只能字符串匹配；与 #19 同源，是统一错误模型的前提。
- **方向**：序列化携带 `kind`；前端建 `ApiError` 类型 + 统一 toast/横幅恢复。

#### 🟡 C3 `Settings` 字段已建模但未接线
- **现状**：`Settings` 有 `default_query_timeout_ms`/`history_retention_days`/`capture_history`/`theme`（`config.rs:52-68`），但全壳层 grep 无任何消费点。
- **影响**：配置项是「摆设」，用户改了不生效；超时/保留策略无落地（`theme` 前端硬编码 oneDark）。
- **方向**：`capture_history` 在历史归因处判断；`history_retention_days` 接后台清理；`theme` 由壳层/前端消费。与 #13/#17 一并规划。

#### 🟡 C4 历史 `clear` 不清理 FTS + 跨项目语句归因有损（扩展 #14）
- **现状**：`clear(Some(p))` 删 `executions`+`statements`，但**不删 `statements_fts`**（`history.rs:355-369`）；`record` 去重命中时用 `project_id=?3` **覆盖**语句归属为最后执行者（`history.rs:216-219`）。
- **影响**：FTS 表累积孤儿行；跨项目共享语句被归到最后一个项目，删某项目历史误删其它项目仍在用的语句。
- **方向**：`clear`/`delete_statement` 同步维护 FTS（contentless 或重建）；语句库 `project_id` 改为多值关联表。

#### 🟡 C5 `ColumnInfo` 缺精度/scale/unsigned/charset/序位（阻塞 #1 修复）
- **现状**：`ColumnInfo` 只有 `name/type_name/nullable/primary_key/default/comment`（`metadata.rs:16-27`），无 `numeric_precision/scale`、`unsigned`、`char_max_length`、`charset/collation`、`ordinal_position`。
- **影响**：#1 修复方向是「按列类型驱动值转换」，但当前元数据只有 `type_name` 字符串（且两路径还不一致，见 D3），无法可靠区分 `DECIMAL(10,2)` 与 `INT`、`BIGINT UNSIGNED` 与 `BIGINT`。
- **方向**：扩展 `ColumnInfo`（结构化列类型），与 #1 的「类型映射层」一起定 schema。

#### 🟢 C6 语句库去重按「字面量规范化」而非参数化
- **现状**：`normalize_sql` 仅折叠空白+去尾分号，`sql_hash` 大小写不敏感（`history.rs:62-76`）；`… WHERE id=1` 与 `… WHERE id=2` 是两条。
- **方向**：可选做「参数化+模板哈希」（模板库/字面量库分层），M2 历史聚合时再定。

#### 🟢 C7 `DangerLevel::Warn` 变体死代码
- **现状**：`Warn` 从未被构造（`danger.rs:7-13,29-59`），只产出 `Safe`/`Dangerous`；前端也只判 `dangerous`。
- **方向**：删 `Warn`，或实现警告级（无 WHERE 的 DELETE/UPDATE 走 Warn）。

#### 🟢 C8 `config.json` 非原子写
- **现状**：`AppConfig::save` 用 `fs::write` 直接覆盖（`config.rs:87-94`）。
- **影响**：崩溃/断电可能损坏配置，首次启动退回「默认项目」并静默丢全部连接。
- **方向**：write-to-temp + `rename` + 可选备份。

#### 🟢 C9 `split_statements` 不支持 dollar-quoting 等方言差异
- **现状**：切分只处理 `'`/`"`/反引号 + `--`/`#`/`/* */`（`dialect.rs:18-114`）。
- **影响**：接入 Postgres 时函数体（含 `;` 的 dollar-quote）被误切，危险分析/语句切分失真。
- **方向**：`split_statements` 并入 `Dialect`（方言感知切分），随 M2 驱动接入实现。

---

## 二、驱动 `dby-driver-mysql`

### 2.1 文档↔代码差异

- `README` 技术栈表写「mysql 28」（已随本评审修正为 `mysql_async 0.37`）；`subsystems.md §1` 已正确写 `mysql_async`。
- `StreamEvent` 含 `Info` 变体，但**驱动从不产出**（`lib.rs` 只 emit `Columns/Rows/Affected`）。
- `capabilities.supports_cancel = true` 语义偏乐观：实际是 drain 式批间取消，与「秒断」无关。
- `Value::Time` 契约（`value.rs:23-24` 定义 `"HH:MM:SS[.ffffff]"`）与驱动实际产出（`"{sign}{days} HH:MM:SS.ffffff"`）不符。
- 集成测试注释过时（`mysql_integration.rs:214` 写「递归 CTE」，实际 SQL 是交叉连接 + `SLEEP(0.001)`）。

### 2.2 新增设计缺口

#### 🔴 D1 值转换启发式（确认并细化 defects #1）
- **现状**：`mysql_value_to_dby`（`conv.rs:10-37`）：`Bytes` 可 UTF-8 解码即 `Str`（DECIMAL 因此落为 `Str`，**BLOB/BINARY 偶发被 UTF-8 误判为 `Str`**）；`Date(y,mo,d,h,mi,s,us)` 靠 `h==0&&mi==0&&s==0&&us==0` 判 `Date`，否则 `DateTime` → **午夜 DATETIME 误判为 Date**；全程无时区信息（TIMESTAMP/DATETIME 塌缩）。且 `Decimal/Bool/Json/Uuid/Array/Map` 六个 `Value` 变体在本驱动**从不产出**（JSON 列恒 `Str`）。
- **影响**：类型着色/编辑控件/导出失真；与 #1 完全一致，本条目附精确 file:line 作为修复锚点。
- **方向**：按 `ColumnInfo.type_name` 驱动转换——BLOB/BINARY→`Bytes`、JSON→`Json`、DECIMAL→`Decimal`、`TINYINT(1)`→可选 `Bool`，明确 `Uuid/Array/Map` 归属。

#### 🟢 D2 `Time` 值格式失真
- **现状**：`MyValue::Time(neg,days,h,mi,s,us)` 恒格式化为 `"{sign}{days} {h:02}:{mi:02}:{s:02}.{us:06}"`（`conv.rs:32-35`），即 `us==0` 也带 `.000000`，负值/超 24h 表示与「HH:MM:SS」约定不一致。
- **方向**：按 `us` 是否为零决定小数位；统一 TIME 表示。

#### 🟡 D3 `ColumnInfo.type_name` 两条路径不一致
- **现状**：元数据路径 `columns()` 取原始 `COLUMN_TYPE`（如 `"int(11) unsigned"`、`"decimal(10,2)"`，`lib.rs:145`）；查询结果路径 `execute_stream` 取 `display_type_name(Debug)`（如 `"long"`、`"newdecimal"`，`lib.rs:269`）。
- **影响**：同一列在不同来源下 `type_name` 不同（`int` vs `long`），长度/精度/unsigned 全丢；前端类型显示不一致、类型映射层（#1）无法依赖。比 #20（仅「依赖 Debug 脆弱」）更直接。
- **方向**：两条路径统一到同一「列类型」表示（结构化，见 C5）。

#### 🟡 D4 DML 不可取消（补充 #5）
- **现状**：`execute_stream` 的取消检查只在 `if let Some(cols)`（SELECT 分支）内（`lib.rs:284-288`）；非 SELECT 的 `Affected` 分支无任何取消检查。
- **影响**：长事务性 UPDATE/DELETE 无法取消；叠加 S1（锁）后「取消」整体失效。
- **方向**：取消检查覆盖 DML；与 #5「真·秒断（关连接）」一并设计。

#### 🟡 D5 流式忽略 `max_rows` + 每查询 `USE`
- **现状**：`execute_stream` 完全无视 `opts.max_rows`，无界推送所有行（`lib.rs:247-300`）；且每条 SQL 前 `query_drop("USE db")`（`lib.rs:254-261`）改变连接当前库。
- **影响**：前端无界（见 F3）+ 连接态副作用（并发/重入风险）；`USE` 对含库名前缀的 SQL 是冗余。
- **方向**：后端按 `max_rows` 截断并推 `truncated`；`USE` 改为按需/可选。

#### 🟢 D6 元数据 SQL 字符串拼接
- **现状**：`indexes`（`SHOW INDEX FROM {q} FROM {q}`）、`table_ddl`（`SHOW CREATE TABLE {q}.{q}`）用 `format!` 拼标识符（`lib.rs:155-159,235-239`）。
- **影响**：已 `quote_identifier` 转义，注入风险低，但非参数化、易漏。
- **方向**：统一参数化元数据查询；`table_ddl` 无法参数化的例外显式标注。

#### 🔴 D7 SSH 隧道断开后资源泄漏（任务/会话/临时端口不释放）
- **现状**：`tunnel.rs:62-85` 的 accept 循环是 `tokio::spawn` 出来的任务，闭包持有 `listener` 和 `h`（`Arc<Handle>` 克隆）；`MysqlConnection` drop 时 `SshTunnel` 只 drop 了 `JoinHandle`（**detach，不 abort**）和自己的 `Arc` 克隆，但循环任务仍持有最后的 `Arc<Handle>` + `listener`，`accept()` 永不返回 Err，**循环永不退出**。
- **影响**：每次「连接→断开」泄漏 1 个 SSH 会话 + 1 个本地临时端口 + 1 个 tokio 任务；反复重连耗尽端口/FD/内存，SSH 服务端堆积连接（`MaxSessions` 到限后拒连）。
- **方向**：隧道引入可取消机制（`CancellationToken`/`Notify`），`Drop` 时 abort 任务并显式关闭 listener。

#### 🟡 D8 SSH 无连接超时 + direct-tcpip 失败被吞（诊断差）
- **现状**：`russh::client::connect` 用 `Config::default()` 无超时（`tunnel.rs:39-42`）；内层 `tokio::spawn` 里 `channel_open_direct_tcpip(...).await?` 的错误随 `JoinHandle` 被丢弃（`tunnel.rs:78-79`）。
- **影响**：SSH 主机黑洞/丢包时长时间挂起；目标 MySQL 不可达时只见通用错误，看不到真实根因。
- **方向**：SSH 连接与 direct-tcpip 设超时；捕获内层任务结果并映射为可读 `DbError`。

#### 🟡 D9 `begin` 嵌套隐式提交 + autocommit 无连接侧状态
- **现状**：`begin` 恒发 `START TRANSACTION`（`lib.rs:302-304`），`set_autocommit` 直接 `SET`（`lib.rs:314-321`），`Connection` 无「当前事务/autocommit 状态」字段。
- **影响**：`set_autocommit(0)` 后再 `begin`，`START TRANSACTION` 会**隐式提交**当前事务；重连后 autocommit 复位但前端 workspace 不知情（`subsystems.md:39` 只提「无持久化」）。
- **方向**：连接侧维护事务/autocommit 状态机，重连时同步前端状态。

#### 🟢 D10 tokio `time` feature 依赖传递启用（脆弱）
- **现状**：集成测试用 `tokio::time::sleep`（`mysql_integration.rs:222`），但 dev-dependencies 的 tokio 仅声明 `["macros","rt-multi-thread"]`（`Cargo.toml:19`），`time` 靠 mysql_async/russh 传递开启。
- **方向**：dev-dependencies 显式加 `"time"`。

---

## 三、壳层 `src-tauri`

### 3.1 文档↔代码差异

- `architecture.md §5` 承诺「密码/SSH 凭据 → OS 钥匙串」，但**只有 MySQL 密码进钥匙串**，SSH 密码/私钥明文进 `config.json`（见 S2）。
- `subsystems.md §9`/`ResultsGrid` 声称「2000 行截断」，壳层**从未实现**（所有 `ExecOpts::default()`，`truncated` 恒 false，见 S4）。
- `architecture.md §3` 列了 `Connection` 全套方法，但壳层只暴露 `schemas/tables/columns`，`ping/indexes/foreign_keys/triggers/procedures/table_ddl/catalogs` 无对应 command（见 S13）。
- 前端 `SavedConnection` 类型只声明 8 字段，后端 `list_saved_connections` 返回完整 `ConnectionConfig`（含 `ssl/ssh/color`）——契约漂移（见 S2）。

### 3.2 新增设计缺口

#### 🔴 S1 全局单锁跨 `await` 持有 → 串行化 + 取消失效（比 #5 更根本）
- **现状**：`connections: Mutex<HashMap<u64, ActiveConnection>>` 是**唯一全局锁**；`execute_query_stream`（`commands.rs:457-478`）、`execute_query`（294-308）、`export_result`（769-785）、`list_databases/tables/columns`（251-284）、`begin/commit/rollback/set_autocommit`（522-554）、`execute_edit`（600-614）、`run_ddl`（660-674）都在持有 `guard` 期间 `await` 连接操作。`cancel_query` 也要抢同一把锁（`commands.rs:503-509`）。
- **影响**：**任意**连接上的一次慢查询，串行化**所有**连接的**一切**操作；且查询进行中 `cancel_query` 永远抢不到锁 → **取消按钮在运行中完全无效**（比 #5 的 drain 更前置——根本到不了 drain）。
- **方向**：锁只用于「取出/放回」连接句柄，不跨 `await` 持有；或改为 per-connection 锁（`HashMap<u64, Arc<tokio::sync::Mutex<ActiveConnection>>>` / `DashMap`）。`cancel_query` 只需读 token 的 `Arc`，无需抢连接锁。属架构级，优先修。

#### 🔴 S2 SSH 密码明文落盘 + 经 IPC 返回前端（违反 architecture §5）
- **现状**：`connect` 把 `params.ssh.clone()`（含 `password`、`private_key`）写入 `ConnectionConfig` 并 `save` 到 `config.json`（`commands.rs:88-94`）；仅 MySQL `password` 进钥匙串（96-99）。`list_saved_connections` 返回完整 `Vec<ConnectionConfig>`（174-185），**SSH 密码明文发到前端**；`reconnect` 从 config 取 ssh（210）。
- **影响**：SSH 凭据明文落盘 + 前端可见，违背安全承诺。
- **方向**：SSH 密码/私钥同样进钥匙串（key=`config_id:ssh`）；`SshOptions.password/private_key` 加 `skip_serializing`；`list_saved_connections` 返回脱敏视图。

#### 🔴 S3 取消令牌每连接一个且永不 reset（sticky cancel）
- **现状**：`ActiveConnection.cancel` 建连接时创建一次（`state.rs:19`、`commands.rs:123`），`execute_query_stream` 每次克隆同一 `Arc<AtomicBool>`（464-466），`cancel_query` 置 true（506），**全仓库无 reset**（`CancellationToken` 无 `reset()`，`query.rs:105-117`）。
- **影响**：修好 S1 后，一次取消会使该连接**后续所有查询**在首批后立即 `Cancelled`，连接被「毒化」直到重连。
- **方向**：取消令牌**按查询实例**创建（每次 `execute_*` 新建 token，登记「查询 id→token」）；或 token 加 `reset()`，查询开始前重置。

#### 🟡 S4 导出无行数上限、整串过 IPC
- **现状**：`export_result` 用 `ExecOpts::default()` 全量收集进内存（`commands.rs:777-783`），格式化后作为**一个 String** 返回前端剪贴板（809-823）。
- **影响**：大结果集内存爆炸 + IPC 巨串；与 subsystems §9 文档描述相反。
- **方向**：导出改为流式写文件（M2「大结果集完整导出」），前端拿文件句柄而非字符串。

#### 🟡 S5 历史/钥匙串写入错误被吞
- **现状**：所有命令 `let _ = state.history.record(&rec)`（`commands.rs` 多处）、`set_secret`/`delete_secret` 失败 `let _`（98,232），历史/凭据写失败静默。
- **影响**：SQLite 锁/满时执行「成功」但历史丢失；删除连接时钥匙串残留。与 #13 同源（#13 是同步写，此处是错误吞噬，独立问题）。
- **方向**：后台 writer（#13 方案）+ 失败至少 log/告警，不阻断查询但不静默。

#### 🟡 S6 `USE db` 每查询执行一次（连接态副作用，同 D5）
- **现状**：`execute_stream` 在每条 SQL 前 `USE {db}`（`driver lib.rs:254-261`），改变连接当前库。
- **影响**：多标签/并发下库上下文漂移；含库名前缀的 SQL 冗余。
- **方向**：见 D5，`USE` 改为按需/可选。

#### 🟡 S7 `delete_project` 只校验活跃连接，遗留孤儿配置与钥匙串
- **现状**：`delete_project` 仅检查 `state.connections`（活跃，`commands.rs:372-376`），不检查 `config.connections`（已保存）。删除后同项目的 `ConnectionConfig` + keyring 密码**原样保留**。
- **影响**：孤儿配置在 `list_saved_connections`（按 project_id 过滤）永远不可见、无法删除，凭据泄漏在钥匙串；`resolve_project_id` 空串兜底还会写出 `project_id:""`。
- **方向**：删除项目时级联校验/清理 `config.connections`（拒绝或提示级联），并删除对应 keyring 条目。

#### 🟡 S8 流式通道无终止/错误事件 + `channel.send` 失败被吞
- **现状**：`StreamEvent` 无 `Done`/`Error`/`End`（`query.rs:91-96`）；`ChannelSink.on_event` 忽略 `channel.send` 结果（`commands.rs:444`）。命令最终成功/失败走 `invoke` 返回值，与 channel 是两条独立传输。
- **影响**：前端无法可靠判断「最后一批」与「命令返回」顺序，收尾存在竞态；前端关标签/销毁 channel 后，后端仍继续执行并静默丢弃结果，慢查询变无主孤儿。
- **方向**：channel 上发终止事件（`Done`/`Error`）；`channel.send` 失败时主动触发取消/中断。

#### 🟡 S9 `connect` 半失败：连接已建但 config 保存失败时返回 Err 却留孤儿连接
- **现状**：`connect` 先 `open_session`（建连 + `connections.insert`，`commands.rs:126`），再 `cfg.save(...)?`（94）。保存失败 `?` 返回 Err，但连接仍留在表里。
- **影响**：前端收到失败却存在「幽灵连接」，无法通过 UI 关闭（`list_connections` 仍列出）。
- **方向**：保存失败时回滚（`disconnect` 该连接）或先保存后建连。

#### 🟡 S10 启动加载健壮性：config 损坏静默重置、history 打不开 panic
- **现状**：`AppConfig::load` 失败被 `unwrap_or_else` 静默重置（`lib.rs:22-23`）；`HistoryStore::open` 失败 `.expect` 崩溃（`lib.rs:24-25`）。
- **影响**：config 损坏（与 C8 非原子写叠加）→ 全部项目/连接无提示丢失；history.db 占用/损坏 → 应用无法启动。
- **方向**：config 损坏时备份原文件并提示；history 打开失败降级为内存态/只读并告警。

#### 🟡 S11 数值精度：`I64/U64` 与 id/last_insert_id 以 JSON number 跨 IPC，>2^53 丢精度
- **现状**：`Value::I64/U64`、`ConnectResponse.id: u64`、`QueryOutput.last_insert_id: Option<u64>` 序列化为 JSON 数字（`value.rs:13-14`、`commands.rs:22`、`query.rs:77`），前端以 JS `number` 接收。
- **影响**：BIGINT / BIGINT UNSIGNED（MySQL 常见主键）超 2^53 静默丢精度，展示/编辑错误。与 #1 相关但独立——即使类型映射做对，精度仍会丢。
- **方向**：`I64/U64` 跨 IPC 以字符串承载（或带 `s` 字段），前端 BigInt/字符串渲染；`id/last_insert_id` 改字符串。

#### 🟡 S12 `export_result` 在 format 校验前就执行查询并记「成功」历史
- **现状**：`export_result` 先 `execute_buffered` + `history.record`（origin=export、status=ok，`commands.rs:769-807`），之后才 match format（813-822）。
- **影响**：非法 format 先白执行一次查询，且历史留下「成功导出」而实际失败。
- **方向**：format 前置校验后再执行；历史 status 反映最终结果。

#### 🟡 S13 命令面不完整：`Connection` 能力无对应 command
- **现状**：`Connection` trait 声明 15 方法（`driver.rs:111-138`），壳层只暴露 `schemas/tables/columns`；`Capabilities.supports_procedures` 等能力位前端无命令支撑。
- **影响**：能力矩阵「落空」，前端无法做索引/外键/存储过程/表 DDL 视图。
- **方向**：补齐元数据命令（至少 `ping`、`indexes`、`foreign_keys`、`table_ddl`）或明确标注「未实现」并在文档降级。

#### 🟢 S14 连接配置无更新命令、`color` 死字段、驱动参数不持久化
- **现状**：`connect` 每次生成新 uuid 追加配置（`commands.rs:77`），无 update；`ConnectionConfig.color` 恒 `None`；`ConnectParams.params` 无对应字段、`reconnect` 置空（`commands.rs:211`）。
- **影响**：重复连接产生重复保存项（叠加 #6）；未来驱动参数重连时丢失。
- **方向**：增加 `update_saved_connection`；明确 `color`/`params` 存续语义。

#### 🟡 S15 安全面：`csp: null` + 自定义 command 无 ACL 门控
- **现状**：`tauri.conf.json:23` `"csp": null`；`capabilities/default.json` 仅 `"core:default"`，自定义命令（含 `drop_database`/`drop_table`/`delete_project` 等破坏性命令）对 WebView 全量开放，无 per-command ACL。
- **影响**：一旦 XSS（CSP 关闭放大），攻击面覆盖全部破坏性命令与凭据读取（`list_saved_connections` 还泄 SSH 密码，见 S2）。
- **方向**：启用 CSP；破坏性命令加服务端二次确认；`list_saved_connections` 返回脱敏视图。

---

## 四、前端 `src`

### 4.1 文档↔代码差异

- `architecture.md:57` 称「`mutateResult` 就地追加 + **版本号触发渲染**」——`resultVersion` **只写不读**（死字段），实际靠返回新 `workspaces` 对象引用触发渲染。
- `subsystems.md:19` 称「切项目 `setActive(null)` 丢活动连接」——实际 tabs **不按项目过滤**，切项目后顶部仍残留跨项目标签。
- `architecture.md §6` 对导出数据流描述不精确：导出走独立 `export_result`（返回 string），非 `QueryOutput`。

### 4.2 新增设计缺口

#### 🔴 F1 模态框（编辑/危险确认）不绑定连接，跨 tab 执行错位
- **现状**：`pendingEdit`/`pendingDanger` 只存 sql/pk/set/table/database，**不含 connId**（`App.tsx:44-56`）；确认时用当前 `activeId` 执行。
- **影响**：弹窗打开期间切标签 → 编辑/危险 SQL 落到**错误连接**，可能改错库。
- **方向**：状态携带 `connId`，确认时用记录 id；或弹窗打开时锁定切换。

#### 🔴 F2 数据编辑用 `selectedTable`+结果列当主键，未校验结果来源
- **现状**：`handleEditCell` 用 `ws.selectedTable` 当表名、`rs.columns` 的 `primary_key` 当主键直接 `buildEditSql`（`App.tsx:253-280`）；`selectedTable` 仅在 `handleOpenTable` 设置。
- **影响**：手写 JOIN/自定义 SELECT 后双击单元格 → 用错误表名/主键生成 UPDATE，可能误改数据。
- **方向**：结果与 `(database, table, 主键列)` 显式绑定（仅表浏览结果可编辑）。

#### 🔴 F3 流式 `truncated`/`info` 未接线 + 行追加无上限
- **现状**：`truncated` 恒 false、`StreamEvent` 无 truncated 事件（`App.tsx:143-149`）；`case "info": break` 静默丢弃（170-171）；`rows.push(...)` 无上限（159-163）。
- **影响**：「已截断，仅显示前 2000 行」永远不显示；服务端 info/warning 丢失；无界行增长 → 内存/虚拟滚动压力。
- **方向**：后端截断并推 `truncated`；`info` 落状态栏；前端 `rows.length` 软上限。

#### 🟡 F4 connect/reconnect 依赖「列表末尾即最新」启发式
- **现状**：`finishConnect` 取 `list[list.length-1]`（`App.tsx:87`）；`connect/reconnect` 返回的 `ConnectResponse.id` 被丢弃。
- **影响**：非严格插入序或 reconnect 复用 id 时会开错连接。
- **方向**：直接用 `connect/reconnect` 返回的 id 打开/定位连接。

#### 🟡 F5 能力矩阵未在前端消费
- **现状**：`Capabilities`/`DriverInfo.capabilities` 已建模，但全 `src/` 无组件读取。
- **影响**：`supports_data_edit=false` 仍可编辑、`supports_transactions=false` 仍显示事务按钮——接新驱动即错。
- **方向**：连接建立后存 capabilities，据以开关编辑/事务/取消/导出控件。

#### 🟡 F6 `analyze_danger` 的 `warn` 级被忽略
- **现状**：`DangerLevel` 有 `warn` 变体，`handleRun` 只判 `dangerous`（`App.tsx:190-194`）。
- **方向**：`warn` 也弹提示（可带「不再提示」），或明确处置策略。

#### 🟡 F7 历史搜索无防抖
- **现状**：`refreshStatements` 依赖 `query`，每键触发 FTS（`HistoryPanel.tsx:16-26,39-42`）。
- **方向**：输入防抖 ~300ms + 仅 Enter/失焦触发。

#### 🟡 F8 启动不 `listConnections()`，前端重载后丢失活动连接
- **现状**：挂载 effect 只 `listDrivers/listProjects/listSavedConnections`，**从不 `listConnections`**（`App.tsx:62-73`）。
- **影响**：HMR/前端重载后 `connections=[]`，UI 无法操作后端仍存活的连接。
- **方向**：挂载时 `listConnections()` 回填 + 重建 tabs/workspaces。

#### 🟡 F9 tabs 与项目过滤不一致（跨项目标签残留）
- **现状**：侧栏按项目过滤（`App.tsx:59`），顶部 `tabs` 遍历全量（399-421）。
- **方向**：tabs 也按项目过滤，或明确「tabs 全局、树按项目」并同步 active 逻辑。

#### 🟡 F10 `String(e)` 错误串化
- **现状**：catch 里 `String(e)`（`App.tsx` 多处），Tauri invoke 拒绝常是对象 → `[object Object]`。
- **方向**：抽 `errToString(e)` 解析 Tauri 错误结构，配合 #19 统一 toast。

#### 🟢 F11 `resultVersion` 只写死字段
- **现状**：`store.ts:114` 自增，全项目无读取。
- **方向**：删除，或改用 `useStore(selector)` 真正按版本订阅。

#### 🟢 F12 `mutateResult` 在 `set` 内就地 mutate
- **现状**：`fn(ws.result)` 直接改传入对象（`store.ts:106-117`）。
- **方向**：返回新数组/新对象（不可变更新），避免 StrictMode 重放重复 push。

#### 🟢 F13 SchemaTree 展开无「已加载」短路，缓存无失效
- **现状**：每次展开都 `loadChildren`（`SchemaTree.tsx:61-68`）；`dbs/tables/columns` 只增不删。
- **方向**：加 loaded 标记 + 显式失效/淘汰。

#### 🟢 F14 全局 `status` 无 per-connection 语义
- **现状**：`status` 是 App 单例（`App.tsx:48`），多连接切换后显示最近一次查询结果。
- **方向**：status 移入 workspace，或显示时绑定活动连接。

#### 🟢 F15 可访问性缺失
- **现状**：树节点是无语义 `div`（无 role/tabIndex）、大量 emoji 按钮缺 `aria-label`、网格非语义表格。
- **方向**：树加 `role=tree/treeitem` + 焦点管理；图标按钮补 `aria-label`。

#### 🟢 F16 编辑控件类型覆盖不足（补充 #11）
- **现状**：`toCellValue` 只产出 null/i64/f64/str（`App.tsx:14-20`）；decimal/date/time/datetime/json/bytes/bool/uuid/array/map 一律按字符串；u64 > `MAX_SAFE_INTEGER` 被 i64 化丢精度、decimal `"1.50"` 变 f64 丢精度。
- **方向**：提交时携带列 `type_name`，由后端类型映射层解析（与 #1/#11 合并方案）。

---

## 五、跨层汇总

| 严重度 | 新增缺口 | 与既有 defects 的关系 |
| --- | --- | --- |
| 🔴 | S1（全局锁跨 await）、S2（SSH 明文）、S3（sticky cancel）、D7（SSH 隧道泄漏）、F1（模态框跨 tab）、F2（编辑来源未校验）、F3（流式无界） | S1/S3 比 #5 更根本；D1 细化 #1；D7/D8 覆盖 #8/#9 |
| 🟡 | C1–C5、D3–D5、D8–D9、S4–S13、S15、F4–F10 | C4 扩展 #14；C3 与 #13/#17 同源；S11 与 #1 相关独立；F16 补充 #11 |
| 🟢 | C6–C9、D2、D6、D10、S14、F11–F16 | F11 补充 #15（`resultVersion` 也是死字段） |

**评审优先级建议**：`S1 → S2 → S3` 构成「安全 + 核心体验」第一梯队且彼此耦合（取消语义）；`D7`（SSH 泄漏）与 `S2`（SSH 明文）同属 SSH 安全面应一起修。建议与既有架构级缺陷 #9/#5/#4/#1 合并为「一次性定方案」的首批评审对象。

## 六、评审素材（供完善设计文档使用）

- **完整 IPC 契约（36 命令）**：参数/返回形状与 4 处契约漂移（SSH 密码泄漏、number 精度、`warn` 死变体、流式无终止事件）已由壳层子代理整理，可直接并入 `architecture.md` §6。
- **集成测试覆盖缺口**：SSH/TLS 完全无测试；`columns/indexes/foreign_keys/triggers/procedures/table_ddl/catalogs` 未在集成测试断言；取消仅验证 drain 语义未验证秒断；`Time`/DECIMAL/JSON/TINYINT(1)/大结果集/多结果集/错误路径/`last_insert_id` 未测。可直接并入 `subsystems.md` 的测试策略。
