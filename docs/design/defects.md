# 已知设计缺陷清单（评审核心）

按严重度分组：🔴 高（安全/正确性/核心体验）→ 🟡 中（一致性/可维护性）→ 🟢 低（代码债）。

每项：现状 → 影响 → 建议修复方向。评审确认后据此统一修复。

> 首批 20 项（#1–#20）为回溯性设计记录时的已知缺陷；**评审新增 49 项（#21–#69）** 见文末「评审新增缺陷」一节，完整证据（file:line）见 [`review.md`](./review.md)。

---

## 缺陷分档总表（优先级 P 级 + 任务规模 + 执行档位）

> 每条缺陷统一标注 **优先级（P0–P3）** 与 **任务规模（大/中/小）**，并按执行档位分三组。此表为评审与排期的**权威索引**。
>
> **优先级**：`P0` 阻塞/安全紧急 > `P1` 高 > `P2` 中 > `P3` 低（P0/P1 ≈ 🔴，P2 ≈ 🟡，P3 ≈ 🟢）。
> **档位**：A 直接修（小）· B 需评审（中）· C 需统一方案（大），对应铁律的分规模执行。

### 档位 C —— 需统一方案（大）

> 架构级，先 `brainstorming`/`writing-plans` 定方案 → 评审通过 → 按独立模块拆多子代理并行。
> **状态：10 个方案组已全部实现、评审通过并合并进 `master`（TDD + CI 门禁 + 集成测试全绿）。**

| # | 缺陷 | P | 规模 | 状态 |
|---|---|---|---|---|
| #9 | SSH 隧道接受任意主机密钥（MITM） | P0 | 大 | ✅ 已修复 |
| #22 | SSH 密码/私钥明文落盘 + 经 IPC 返回前端 | P0 | 大 | ✅ 已修复 |
| #48 | `csp:null` + 破坏性命令无 ACL 门控 | P0 | 大 | ✅ 已修复 |
| #1 | Value 类型映射启发式（含 #20/#32/#33/#60） | P1 | 大 | ✅ 已修复 |
| #4 | 前端硬编码 MySQL 反引号（方言感知） | P1 | 大 | ✅ 已修复 |
| #5 | 取消 drain 式（含 #21/#23/#34） | P1 | 大 | ✅ 已修复 |
| #24 | SSH 隧道断开后资源泄漏 | P1 | 大 | ✅ 已修复 |
| #28 | 多结果集无边界事件（协议层） | P1 | 大 | ✅ 已修复 |
| #45 | BIGINT/主键 >2^53 跨 IPC 丢精度 | P1 | 大 | ✅ 已修复 |
| #18 | 无 i18n（框架先行） | P3 | 大 | ✅ 已修复 |

### 档位 B —— 需评审（中）

> 单条功能/缺陷，先写简短方案 → 评审通过 → 单代理串行或小规模并行。

| # | 缺陷 | P | 规模 |
|---|---|---|---|
| #25 | 编辑/危险确认模态框不绑定连接 | P1 | 中 |
| #26 | 数据编辑来源未校验（可误改数据） | P1 | 中 |
| #44 | 启动加载健壮性（config 重置/history panic） | P1 | 中 |
| #2 | CRUD 用 `window.prompt/confirm` | P2 | 中 |
| #3 | 树节点 key 用 `:` 拼接再 split | P2 | 中 |
| #6 | 连接自动持久化、无「保存连接」开关 | P2 | 中 |
| #8 | SSH 仅密码认证 | P2 | 中 |
| #11 | 数据编辑值类型在前端正则解析（依赖 #1） | P2 | 中 |
| #12 | 危险操作关键词误报 | P2 | 中 |
| #13 | 历史写入同步 | P2 | 中 |
| #14 | 语句库删除未实现 | P2 | 中 |
| #19 | 错误处理不一致 | P2 | 中 |
| #27 | 流式 truncated/info 未接线 + 行追加无上限 | P2 | 中 |
| #29 | `DbError` 跨 IPC 丢失错误种类 | P2 | 中 |
| #30 | `Settings` 字段已建模但未接线 | P2 | 中 |
| #31 | 历史 `clear` 不清理 FTS + 跨项目归因有损 | P2 | 中 |
| #35 | 流式忽略 `max_rows` + 每查询 `USE` | P2 | 中 |
| #36 | SSH 无连接超时 + direct-tcpip 失败被吞 | P2 | 中 |
| #37 | `begin` 嵌套隐式提交 + autocommit 无状态 | P2 | 中 |
| #38 | 导出无行数上限、整串过 IPC | P2 | 中 |
| #41 | `delete_project` 遗留孤儿配置与钥匙串 | P2 | 中 |
| #42 | 流式通道无终止/错误事件 + `send` 失败被吞 | P2 | 中 |
| #47 | 命令面不完整（`Connection` 能力无 command） | P2 | 中 |
| #50 | 能力矩阵未在前端消费 | P2 | 中 |
| #69 | 编辑控件类型覆盖不足（依赖 #1） | P2 | 中 |
| #7 | 关闭标签 = 断开连接 | P3 | 中 |
| #56 | 语句库去重按字面量而非参数化 | P3 | 中 |
| #59 | `split_statements` 不支持方言差异 | P3 | 中 |
| #68 | 可访问性缺失 | P3 | 中 |

### 档位 A —— 直接修（小）

> 单文件/局部改动，无需评审，直接改代码 + 测试。

| # | 缺陷 | P | 规模 |
|---|---|---|---|
| #17 | 查询超时未接线 | P2 | 小 |
| #40 | `USE db` 每查询执行（连接态副作用） | P2 | 小 |
| #43 | `connect` 半失败留孤儿连接 | P2 | 小 |
| #49 | connect/reconnect 依赖「列表末尾」启发式 | P2 | 小 |
| #53 | 启动不 `listConnections()` | P2 | 小 |
| #58 | `config.json` 非原子写 | P2 | 小 |
| #63 | 连接配置无更新命令 / `color` 死字段 | P2 | 小 |
| #66 | SchemaTree 缓存无失效 | P2 | 小 |
| #70 | 视图同时出现在表和视图节点下 | P2 | 小 |
| #71 | 切换项目后保存的连接不显示（R11 未完全实现） | P1 | 中 |
| #72 | reconnect 静默吞没 keyring 错误导致密码丢失 | P1 | 小 |
| #73 | 保存的连接无法删除（R11 UI 缺失） | P2 | 小 |
| #74 | 双击占位符连接（未激活的保存连接）报错 | P2 | 小 |
| #75 | Schema 树缺少创建表/视图/函数/存储过程的右键菜单 | P2 | 中 |
| #10 | 结果网格列宽固定 | P3 | 小 |
| #15 | workspace 残留未用字段 | P3 | 小 |
| #16 | 切换项目丢失活动连接 | P3 | 小 |
| #39 | 历史/钥匙串写入错误被吞 | P3 | 小 |
| #46 | `export_result` format 校验前执行查询 | P3 | 小 |
| #51 | `analyze_danger` 的 `warn` 级被忽略 | P3 | 小 |
| #52 | 历史搜索无防抖 | P3 | 小 |
| #54 | tabs 与项目过滤不一致 | P3 | 小 |
| #55 | `String(e)` 错误串化 | P3 | 小 |
| #57 | `DangerLevel::Warn` 死代码 | P3 | 小 |
| #61 | 元数据 SQL 字符串拼接 | P3 | 小 |
| #62 | tokio `time` feature 未显式声明 | P3 | 小 |
| #64 | `resultVersion` 死字段 | P3 | 小 |
| #65 | `mutateResult` 就地 mutate | P3 | 小 |
| #67 | 全局 `status` 无 per-connection 语义 | P3 | 小 |

---

## 🔴 高

### #9 SSH 隧道接受任意主机密钥（安全）✅ 已修复
- **现状**：`tunnel.rs` 的 `check_server_key` 恒返回 `true`。
- **影响**：存在中间人攻击风险，违背"生产级安全"。
- **方向**：首次连接展示主机指纹并要求确认（TOFU）；持久化已信任的指纹；或提供"已知主机"校验。

### #5 取消是 drain 式（核心体验）✅ 已修复
- **现状**：取消在「批间检查」触发，返回 `Cancelled` 后 drop 结果流会**排空剩余行**，慢查询要排空很久才返回。
- **影响**：「停止」不即时；取消慢查询形同虚设。
- **方向**：真·秒断 = 取消时关闭底层连接（drop `Conn`）中断服务端查询；代价是连接不可复用，需在下一次使用时重连（或引入连接池/重连逻辑）。

### #1 Value 类型映射启发式（正确性）✅ 已修复
- **现状**：MySQL 文本协议下，DECIMAL 被当 `Str`（依赖字节串）；`Date`/`DateTime` 靠「时分秒是否全 0」区分，**午夜 DATETIME 会被误判为 Date**，且丢弃时区/微秒部分不完整。
- **影响**：结果网格类型着色/编辑控件选择不准确；跨库（未来驱动）更不通用。
- **方向**：`Connection` 元数据已含列类型（`ColumnInfo.type_name`），改为**按列类型驱动**的值转换（真正的「类型映射」层），而非仅凭 Value 变体推断。

### #4 前端硬编码 MySQL 反引号（违背架构承诺）✅ 已修复
- **现状**：`App.handleOpenTable` 生成 `` `SELECT * FROM \`表\` LIMIT 100` `` 硬编码反引号；列浏览/编辑 SQL 也隐含 MySQL 假设。
- **影响**：接 PostgreSQL/SQLite 时查询生成出错，违背「方言感知」设计。
- **方向**：SQL 生成统一收口到 `dby-core`（走 `Dialect.quote_identifier`），前端只传结构化参数（连接 id + 表名），由壳层用驱动方言生成。

---

## 🟡 中

### #2 CRUD 用 window.prompt/confirm（一致性）
- **现状**：项目重命名/删除、表重命名/删除、建库名等用原生 `prompt/confirm`；仅建表用了自定义弹窗。
- **影响**：UI 风格割裂、不可控、无校验、与「生产级」不符。
- **方向**：统一「输入弹窗 / 确认弹窗」组件，替换所有 prompt/confirm。

### #3 树节点 key 用 `:` 拼接再 split（脆弱）✅ 已修复
- **现状**：`c:5` / `d:5:库名` / `t:5:库名:表名`，靠 split + slice(join) 还原。
- **影响**：边界场景易错（多连接同名、特殊分隔符），后续加节点类型更乱。
- **方向**：节点身份改为结构化对象（或稳定编码如 JSON/URL 编码），避免字符串拼接解析。

### #6 连接自动持久化、无「保存连接」开关
- **现状**：每次 `connect` 都写 `ConnectionConfig` + 密码进钥匙串，无法选择不保存。
- **影响**：一次性连接也落盘；凭据留存超出预期。
- **方向**：连接对话框加「保存连接/记住密码」开关；connect 命令按需持久化。

### #7 关闭标签 = 断开连接
- **现状**：标签栏 `×` 调 `handleDisconnect`（断开 + 移除）。
- **影响**：无法「仅关闭标签、保留后台连接」；多连接场景体验受限。
- **方向**：区分「关闭标签」（保留连接会话，可重新打开）与「断开连接」（释放）。

### #8 SSH 仅密码认证
- **现状**：`SshOptions.private_key` 已建模但未实现（russh 私钥解析未接）。
- **影响**：无法用密钥登录（很多生产环境禁密码）。
- **方向**：接入 `russh-keys` 解析私钥 → `authenticate_publickey`。

### #11 数据编辑值类型在前端正则解析
- **现状**：`toCellValue` 用正则判 NULL/整数/浮点/字符串，不感知列类型。
- **影响**：数字列输入可能落成字符串（靠 MySQL 隐式转换）；日期/布尔/JSON 列编辑体验差。
- **方向**：编辑提交时带上列类型，由后端（类型映射层）解析为正确的 `Value`。

### #12 危险操作分析关键词误报
- **现状**：`analyze_danger` 纯关键词匹配，字符串字面量里的 `DROP`/`DELETE` 会误判危险。
- **影响**：误报影响体验；也可能漏报（如 `CREATE` 后跟破坏性操作）。
- **方向**：升级为轻量 SQL tokenizer（或至少跳过字符串/注释内关键词），并扩展危险规则。

### #13 历史写入同步
- **现状**：`HistoryStore.record` 是同步 SQLite 写，在 async 命令里直接调用。
- **影响**：阻塞 tokio worker（当前写入快，量大后显现）；无批处理。
- **方向**：有界通道 + 后台 writer（M1 计划里提过但未实现）。

### #14 语句库删除未实现
- **现状**：只实现 `delete_execution` + `pin`；`delete_statement` 因 FTS5 表删除同步问题搁置。
- **影响**：语句库无法清理；历史面板「删除语句」缺失。
- **方向**：FTS5 用 `content=''`（contentless）或「删除后重建 FTS」或软删除（deleted 标记）解决同步问题。

### #16 切换项目丢失活动连接
- **现状**：`handleSwitchProject` 里 `setActive(null)`。
- **影响**：切换项目后当前编辑/结果上下文被清空，体验差。
- **方向**：切换项目仅过滤可见连接，不重置活动连接（若活动连接不在目标项目再提示）。

### #17 查询超时未接线
- **现状**：`ExecOpts.timeout_ms` 已建模但从未使用。
- **影响**：无超时保护，慢查询只能靠手动停止。
- **方向**：在 `execute_stream`/`execute_buffered` 接入超时（`tokio::time::timeout` 或连接级超时）。

### #18 无 i18n ✅ 已修复
- **现状**：全部文案硬编码中文。
- **影响**：无法国际化（计划提了首版中/英）。
- **方向**：接 i18next，文案 key 化。

---

## 🟢 低

### #10 结果网格列宽固定
- **现状**：列宽固定 200px，无自动列宽/拖拽调整。
- **方向**：按内容自适应列宽 + 拖拽调整（M2）。

### #15 workspace 残留未用字段
- **现状**：`WorkspaceState` 仍含 `databases/tables/columns`，但 SchemaTree 自管缓存后已不读。
- **方向**：清理字段，`WorkspaceState` 只保留 query/result/事务/选中库表。

### #19 错误处理不一致 ✅ 已修复
- **现状**：前端多处 `catch { /* ignore */ }` 或只 `setStatus`，无统一错误提示/恢复。
- **方向**：统一错误模型 + 全局 toast/横幅组件。

### #20 列类型显示名生成脆弱
- **现状**：驱动里 `format!("{:?}", ColumnType)` 再 strip `MYSQL_TYPE_` 前缀 → `display_type_name`。
- **影响**：依赖 Debug 输出，未来驱动不一致。
- **方向**：列类型→展示名收敛到类型映射层，按驱动显式声明。

---

## 评审建议

按上文「分档总表」执行：**档位 C（需统一方案）→ B（需评审）→ A（直接修）**。C 组 10 个方案组（#9/#22/#48/#1/#4/#5/#24/#28/#45/#18）**已全部实现、评审通过并合并**；B 组逐条写简短方案评审；A 组直接修。每批评审 + 修复 + 测试闭环。

### 方案文档索引（档位 C）

> C 组 10 个方案组的 `design.md`（设计文档）+ `plan.md`（实现计划）见 [`plans/README.md`](./plans/README.md)（含跨方案共享契约 S1–S6 与模板）。评审顺序：第一梯队 #9/#22/#48/#1/#5 → 第二梯队 #24/#28/#45/#4 → 第三梯队 #18。

| # | 方案组 | 设计文档 / 实现计划 |
| --- | --- | --- |
| #9 | SSH 主机密钥 TOFU | [`design`](./plans/09-ssh-host-key-tofu/design.md) · [`plan`](./plans/09-ssh-host-key-tofu/plan.md) |
| #22 | SSH 凭据存储 | [`design`](./plans/22-ssh-credential-storage/design.md) · [`plan`](./plans/22-ssh-credential-storage/plan.md) |
| #48 | CSP + ACL 门控 | [`design`](./plans/48-csp-acl/design.md) · [`plan`](./plans/48-csp-acl/plan.md) |
| #1 | Value 类型映射 | [`design`](./plans/01-value-type-mapping/design.md) · [`plan`](./plans/01-value-type-mapping/plan.md) |
| #4 | 方言感知 SQL | [`design`](./plans/04-dialect-aware-sql/design.md) · [`plan`](./plans/04-dialect-aware-sql/plan.md) |
| #5 | 取消秒断 | [`design`](./plans/05-cancel-sec-break/design.md) · [`plan`](./plans/05-cancel-sec-break/plan.md) |
| #24 | SSH 隧道生命周期 | [`design`](./plans/24-ssh-tunnel-lifecycle/design.md) · [`plan`](./plans/24-ssh-tunnel-lifecycle/plan.md) |
| #28 | 多结果集协议 | [`design`](./plans/28-multi-resultset/design.md) · [`plan`](./plans/28-multi-resultset/plan.md) |
| #45 | 数值精度 | [`design`](./plans/45-numeric-precision/design.md) · [`plan`](./plans/45-numeric-precision/plan.md) |
| #18 | i18n | [`design`](./plans/18-i18n/design.md) · [`plan`](./plans/18-i18n/plan.md) |

---

# 评审新增缺陷（#21 起 · 来源 [`review.md`](./review.md)）

> 2025 逐层代码审查新增 49 项，按严重度分组。完整现状/影响/方向与 file:line 证据见 `review.md`（引用如 S1/D7/F2）。评审确认后并入上面的正式清单统一跟踪。

## 🔴 高（新增）

### #21 全局单锁跨 `await` 持有 → 全连接串行化 + 取消失效 ✅ 已修复
- **现状**：`connections` 单把全局锁，所有命令在 `guard` 存活期间 `await` 网络 I/O（`commands.rs` 多处）；`cancel_query` 也抢同一把锁。
- **影响**：一次慢查询串行化所有连接的一切操作；运行中「取消」永远抢不到锁，完全失效（比 #5 的 drain 更前置）。
- **方向**：注册表 + per-connection 锁；`cancel_query` 只读 token 的 `Arc`，不抢连接锁。

### #22 SSH 密码明文落盘 + 经 IPC 返回前端 ✅ 已修复
- **现状**：`connect` 把 `params.ssh`（含密码/私钥）写进 `config.json`，`list_saved_connections` 整表返回前端；仅 MySQL 密码进钥匙串。
- **影响**：违背 architecture §5，SSH 凭据明文落盘 + WebView 可见。
- **方向**：SSH 密码/私钥进钥匙串；`SshOptions.password/private_key` 加 `skip_serializing`；返回脱敏视图。

### #23 取消令牌每连接一个且永不 reset（sticky cancel）✅ 已修复
- **现状**：`ActiveConnection.cancel` 建连接时创建一次，`cancel_query` 置 true 后全仓库无 reset。
- **影响**：修好 #21 后，一次取消使该连接后续所有查询立即 `Cancelled`，连接被「毒化」直到重连。
- **方向**：令牌按查询实例创建；或加 `reset()`，查询开始前重置。

### #24 SSH 隧道断开后资源泄漏 ✅ 已修复
- **现状**：tunnel accept 循环是 `tokio::spawn` 任务，`Drop` 只 detach 不 abort，listener 永不关闭。
- **影响**：每次重连泄漏 1 个 SSH 会话 + 临时端口 + 任务，反复重连耗尽资源、SSH 服务端堆积连接。
- **方向**：隧道引入可取消机制，`Drop` abort 任务并显式关闭 listener。

### #25 编辑/危险确认模态框不绑定连接
- **现状**：`pendingEdit`/`pendingDanger` 不含 connId，确认时用当前 activeId 执行。
- **影响**：弹窗期间切标签 → SQL 落到错误连接，可能改错库。
- **方向**：状态携带 connId；或弹窗打开时锁定切换。

### #26 数据编辑来源未校验
- **现状**：用 `selectedTable` + 结果列 `primary_key` 直接 `buildEditSql`，`selectedTable` 仅表浏览时设置。
- **影响**：手写 JOIN/自定义 SELECT 后双击编辑 → 错误表名/主键生成 UPDATE，误改数据。
- **方向**：结果与 `(database, table, 主键列)` 显式绑定，仅表浏览结果可编辑。

### #27 流式 truncated/info 未接线 + 行追加无上限
- **现状**：`truncated` 恒 false、`case "info": break` 丢弃、`rows.push` 无上限。
- **影响**：「已截断」永远不显示；服务端 warning 丢失；无界行增长（内存/虚拟滚动压力）。
- **方向**：后端截断推 truncated；info 落状态栏；前端 `rows.length` 软上限。

## 🟡 中（新增）

### #28 多结果集无边界事件，缓冲路径错误合并 ✅ 已修复
- **现状**：`StreamEvent` 无「结果集结束」标记；`CollectingSink` 覆盖 columns、所有 rows 追加到同一组；驱动只读首个结果集。
- **影响**：存储过程/多语句的后续结果集被静默丢弃或错误合并。
- **方向**：加 `ResultSetEnd`（或 Columns 带序号）；sink 按边界分桶；驱动遍历所有结果集。

### #29 `DbError` 跨 IPC 丢失错误种类
- **现状**：`DbError` 只序列化 `{"message"}`，variant 不传前端。
- **影响**：前端无法区分「取消」与「失败」，只能字符串匹配。
- **方向**：序列化带 `kind`；前端建 `ApiError` + 统一 toast。

### #30 `Settings` 字段已建模但未接线
- **现状**：`default_query_timeout_ms`/`history_retention_days`/`capture_history`/`theme` 无任何消费点。
- **影响**：配置项是摆设，改了不生效；超时/保留/主题无落地。
- **方向**：`capture_history` 归因处判断；`history_retention_days` 接清理；`theme` 接前端。

### #31 历史 `clear` 不清理 FTS + 跨项目语句归因有损（扩展 #14）
- **现状**：`clear` 删 executions/statements 但不删 FTS；去重命中用最后执行者覆盖 `project_id`。
- **影响**：FTS 孤儿行累积；跨项目共享语句归因丢失，误删。
- **方向**：FTS 同步维护；`project_id` 改多值关联表。

### #32 `ColumnInfo` 缺精度/scale/unsigned/charset/序位（阻塞 #1）
- **现状**：只有 `type_name` 字符串，无结构化列类型。
- **影响**：类型映射层无法可靠区分 DECIMAL/INT、BIGINT/UNSIGNED。
- **方向**：扩展 `ColumnInfo`（结构化 `ColumnType`），与 #1 一起定 schema。

### #33 `ColumnInfo.type_name` 两条路径不一致
- **现状**：元数据路径用 `COLUMN_TYPE`（"int(11) unsigned"），查询结果路径用枚举 Debug（"long"/"newdecimal"）。
- **影响**：同一列不同来源类型名不同，前端显示不一致、类型映射无法依赖。
- **方向**：两路径统一到结构化列类型。

### #34 DML 不可取消（补充 #5）✅ 已修复
- **现状**：取消检查只在 SELECT 分支内，DML 无取消检查。
- **影响**：长 UPDATE/DELETE 无法取消。
- **方向**：取消覆盖 DML；与 #5 秒断一起设计。

### #35 流式忽略 `max_rows` + 每查询 `USE`
- **现状**：`execute_stream` 无视 `max_rows`；每条 SQL 前 `USE db`。
- **影响**：前端无界；连接态副作用。
- **方向**：后端按 `max_rows` 截断推 truncated；`USE` 按需/可选。

### #36 SSH 无连接超时 + direct-tcpip 失败被吞 ✅ 已修复
- **现状**：`russh::client::connect` 用默认配置无超时；内层 spawn 的 `?` 错误被丢弃。
- **影响**：SSH 黑洞时长时间挂起；排障只见通用错误。
- **方向**：SSH 连接/转发设超时；捕获内层错误映射为可读 `DbError`。

### #37 `begin` 嵌套隐式提交 + autocommit 无连接侧状态
- **现状**：`begin` 恒发 `START TRANSACTION`；`set_autocommit` 直接 SET；无事务/autocommit 状态字段。
- **影响**：`set_autocommit(0)` 后再 `begin` 隐式提交；重连后状态漂移。
- **方向**：连接侧维护事务/autocommit 状态机。

### #38 导出无行数上限、整串过 IPC
- **现状**：`export_result` 用 `ExecOpts::default()` 全量收集进内存，格式化后整串返回前端剪贴板。
- **影响**：大结果集内存爆炸 + IPC 巨串；与文档「2000 行截断」相反。
- **方向**：导出改流式写文件（M2），前端拿文件句柄。

### #39 历史/钥匙串写入错误被吞
- **现状**：`history.record`/`set_secret`/`delete_secret` 失败 `let _` 静默。
- **影响**：历史/凭据写失败静默，删除时钥匙串残留。
- **方向**：后台 writer + 失败至少 log/告警。

### #40 `USE db` 每查询执行一次（连接态副作用）
- **现状**：`execute_stream` 每条 SQL 前 `USE`。
- **影响**：多标签/并发库上下文漂移。
- **方向**：与 #35 一并按需/可选。

### #41 `delete_project` 只校验活跃连接，遗留孤儿配置与钥匙串
- **现状**：只查 `connections` 不查 `config.connections`，删除后 `ConnectionConfig` + keyring 原样保留。
- **影响**：孤儿配置不可见、无法删除，凭据泄漏。
- **方向**：级联校验/清理 + 删除对应 keyring 条目。

### #42 流式通道无终止/错误事件 + `channel.send` 失败被吞 ✅ 已修复
- **现状**：`StreamEvent` 无 `Done`/`Error`；`ChannelSink` 忽略 send 结果。
- **影响**：前端收尾竞态；关标签后后端仍执行，慢查询变无主孤儿。
- **方向**：channel 发终止事件；send 失败主动取消。

### #43 `connect` 半失败留孤儿连接
- **现状**：先建连 + insert，再 `cfg.save(...)?`，保存失败返回 Err 但连接仍在。
- **影响**：前端收到失败却存在幽灵连接。
- **方向**：保存失败回滚断开，或先保存后建连。

### #44 启动加载健壮性：config 损坏静默重置、history 打不开 panic
- **现状**：`AppConfig::load` 失败静默重置；`HistoryStore::open` 失败 `.expect` 崩溃。
- **影响**：config 损坏全丢项目/连接无提示；history.db 占用应用无法启动。
- **方向**：config 损坏备份并提示；history 失败降级内存态/只读。

### #45 数值精度：`I64/U64`/id/last_insert_id 以 JSON number 跨 IPC，>2^53 丢精度 ✅ 已修复
- **现状**：BIGINT/主键序列化为 JSON number，前端 `number` 接收。
- **影响**：超 2^53 静默丢精度，展示/编辑错误。
- **方向**：跨 IPC 以字符串承载，前端 BigInt/字符串渲染。

### #46 `export_result` 在 format 校验前执行查询并记「成功」历史
- **现状**：先 execute + record(ok)，再 match format。
- **影响**：非法 format 白执行一次，历史记录与实际不符。
- **方向**：format 前置校验；历史 status 反映最终结果。

### #47 命令面不完整：`Connection` 能力无对应 command
- **现状**：trait 15 方法，壳层只暴露 schemas/tables/columns。
- **影响**：能力矩阵落空，前端无法做索引/外键/存储过程/DDL 视图。
- **方向**：补齐元数据命令或明确标注未实现。

### #48 安全面：`csp: null` + 自定义 command 无 ACL 门控 ✅ 已修复
- **现状**：`tauri.conf.json` `csp: null`；破坏性命令对 WebView 全量开放。
- **影响**：一旦 XSS，攻击面覆盖全部破坏性命令 + 凭据读取。
- **方向**：启用 CSP；破坏性命令服务端二次确认/ACL；`list_saved_connections` 脱敏。

### #49 connect/reconnect 依赖「列表末尾即最新」启发式
- **现状**：`finishConnect` 取 `list[list.length-1]`，返回的 id 被丢弃。
- **影响**：非严格插入序或 reconnect 复用 id 时开错连接。
- **方向**：直接用返回的 `ConnectResponse.id` 定位连接。

### #50 能力矩阵未在前端消费
- **现状**：`Capabilities` 已建模，全前端无组件读取。
- **影响**：`supports_data_edit=false` 仍可编辑、`supports_transactions=false` 仍显示事务按钮。
- **方向**：连接后存 capabilities，据以开关控件。

### #51 `analyze_danger` 的 `warn` 级被忽略
- **现状**：前端只判 `dangerous`，`warn` 直接放行。
- **影响**：警告级提示能力只实现 1/3。
- **方向**：`warn` 也弹提示或明确处置策略。

### #52 历史搜索无防抖
- **现状**：每键触发 FTS 查询。
- **影响**：后端 FTS 被逐键打爆。
- **方向**：输入防抖 + 仅 Enter/失焦触发。

### #53 启动不 `listConnections()`，前端重载后丢失活动连接
- **现状**：挂载只 listDrivers/listProjects/listSavedConnections。
- **影响**：HMR/前端重载后 UI 无法操作后端仍存活的连接。
- **方向**：挂载时 `listConnections()` 回填。

### #54 tabs 与项目过滤不一致（跨项目标签残留）
- **现状**：侧栏按项目过滤，tabs 遍历全量。
- **影响**：切项目后顶部仍显示其它项目标签，语义混乱。
- **方向**：tabs 也按项目过滤，或明确「tabs 全局」并同步 active。

### #55 `String(e)` 错误串化 ✅ 已修复
- **现状**：catch 里 `String(e)`，Tauri 拒绝常是对象。
- **影响**：错误提示 `[object Object]` 不可读。
- **方向**：抽 `errToString(e)` 解析 Tauri 错误结构。

## 🟢 低（新增）

### #56 语句库去重按「字面量规范化」而非参数化
- **现状**：`normalize_sql` 仅折叠空白+去分号，`WHERE id=1` 与 `id=2` 是两条。
- **方向**：可选参数化+模板哈希（M2 历史聚合再定）。

### #57 `DangerLevel::Warn` 变体死代码
- **现状**：`Warn` 从未被构造，前端也不判。
- **方向**：删 `Warn` 或实现警告级。

### #58 `config.json` 非原子写
- **现状**：`fs::write` 直接覆盖。
- **方向**：write-to-temp + rename + 备份。

### #59 `split_statements` 不支持 dollar-quoting 等方言差异 ✅ 已修复
- **现状**：只处理 `'`/`"`/反引号 + `--`/`#`/`/* */`。
- **方向**：并入 `Dialect`（方言感知切分），随 M2 接入。

### #60 `Time` 值格式失真
- **现状**：恒带 `.000000`、`"{sign}{days} HH:MM:SS.ffffff"` 与契约不符。
- **方向**：按 `us` 决定小数位，统一表示。

### #61 元数据 SQL 字符串拼接 ✅ 已修复
- **现状**：`SHOW INDEX FROM`/`SHOW CREATE TABLE` 用 `format!`。
- **方向**：统一参数化；无法参数化的例外显式标注。

### #62 tokio `time` feature 依赖传递启用
- **现状**：集成测试用 `tokio::time`，dev-dependencies 未显式声明 `time`。
- **方向**：dev-dependencies 显式加 `"time"`。

### #63 连接配置无更新命令、`color` 死字段、驱动参数不持久化 ✅ 已修复
- **现状**：`connect` 每次追加新 uuid；`color` 恒 None；`params` 重连时置空。
- **方向**：加 `update_saved_connection`；明确 `color`/`params` 语义。

### #64 `resultVersion` 只写死字段（补充 #15）
- **现状**：自增但无读取。
- **方向**：删除或改用 selector 订阅。

### #65 `mutateResult` 在 `set` 内就地 mutate
- **现状**：直接改传入对象，破坏不可变约定。
- **方向**：返回新数组/新对象。

### #66 SchemaTree 展开无「已加载」短路，缓存无失效
- **现状**：每次展开都 loadChildren，缓存只增不删。
- **方向**：加 loaded 标记 + 显式失效/淘汰。

### #67 全局 `status` 无 per-connection 语义
- **现状**：status 单例，多连接切换后显示最近一次查询结果。
- **方向**：status 移入 workspace。

### #68 可访问性缺失
- **现状**：树节点无语义 div、emoji 按钮缺 aria-label、网格非语义表格。
- **方向**：加 role/tabIndex/aria-label。

### #69 编辑控件类型覆盖不足（补充 #11）
- **现状**：`toCellValue` 只产出 null/i64/f64/str，decimal/date/json/bytes/u64 一律字符串化或丢精度。
- **方向**：提交时携带列类型，由类型映射层解析。

### #70 视图同时出现在表和视图节点下 ✅ 已修复
- **现状**：`tables()` 查询 `information_schema.TABLES` 未过滤 `TABLE_TYPE`，返回包含视图（`TABLE_TYPE='VIEW'`）。
- **影响**：Schema 树中视图同时出现在「表」和「视图」两个分组下，导致 UI 混乱。
- **方向**：`tables()` 添加 `TABLE_TYPE = 'BASE TABLE'` 过滤条件，仅返回真实表。

### #71 切换项目后保存的连接不显示（R11 未完全实现） ✅ 已修复
- **现状**：前端从未调用 `listSavedConnections`，SchemaTree 仅显示 `listConnections()` 返回的活动连接；切换项目后，新项目的保存连接不可见。
- **影响**：用户切换项目后看到空白连接列表，无法重连已保存的连接，违背 R11「断开不删除、可复用」的设计目标。
- **方向**：App.tsx 启动时和切换项目时调用 `listSavedConnections(projectId)`，合并活动连接与保存连接展示；SchemaTree 根据 `config_id` 显示「打开连接」或「关闭连接」菜单。

### #72 reconnect 静默吞没 keyring 错误导致密码丢失 / Windows keyring v3 bug ✅ 已修复
- **现状**：
  1. `reconnect` 中 `get_secret(...).ok()` 静默吞没所有 keyring 错误
  2. **根本原因**：`keyring 3.6.3` 在 Windows 上存在严重 bug：用同一个 `Entry` 实例可以读回密码，但用新的 `Entry` 实例读取返回 `NoEntry`
- **影响**：Windows 用户重连时密码总是丢失，得到 "using password: NO" 认证错误；`connect` 写入密码成功，但 `reconnect` 创建新 `Entry` 实例读取时失败。
- **方向**：升级到 `keyring = "4"` 修复 Windows 上的读取 bug；增加错误日志便于排障。
- **证据**：`src-tauri/tests/keyring_test.rs:test_keyring_roundtrip` 在 Windows 上复现问题。
- **修复**：升级到 `keyring 4.1.6`，测试确认新 Entry 实例可以正确读取密码。

### #73 保存的连接无法删除（R11 UI 缺失）✅ 已修复
- **现状**：后端有 `delete_saved_connection` 命令，前端 API 也有 `deleteSavedConnection` 方法，但 SchemaTree 连接节点的右键菜单没有"删除连接"选项。
- **影响**：用户无法删除不需要的保存连接，连接列表越来越长；唯一删除方式是手动编辑 config.json。
- **方向**：在 SchemaTree 连接节点右键菜单中添加"删除连接"选项（仅对有 `config_id` 的保存连接显示）；调用 `deleteSavedConnection` 并刷新连接列表。
- **修复**：
  1. `src/App.tsx:467-483` - 添加 `handleDeleteConnection` 函数，删除后刷新保存的连接列表
  2. `src/App.tsx:607` - 传递 `onDeleteConnection` prop 给 SchemaTree
  3. `src/components/SchemaTree.tsx:21+71` - 添加 `onDeleteConnection` prop
  4. `src/components/SchemaTree.tsx:456-458` - 在右键菜单中添加"删除连接"选项（仅对有 config_id 的连接显示）
  5. `src/locales/zh-CN.json` / `src/locales/en-US.json` - 添加翻译（`tree.menu.deleteConnection`、`app.confirm.deleteConnection`、`app.status.connectionDeleted`）

### #74 双击占位符连接（未激活的保存连接）报错 ✅ 已修复
- **现状**：#71 修复引入的回归。占位符连接使用 `id = -1` 表示未激活的保存连接，双击时 `handleSelectConnection` 尝试 `openConnection(-1)`，导致后端 API 调用失败："invalid value: integer `-1`, expected u64"。
- **影响**：用户双击未激活的保存连接时报错，只能通过右键菜单重连。
- **方向**：`handleSelectConnection` 检查 `id === -1`，自动调用 `handleReconnect(config_id)` 而不是 `openConnection`。
- **修复**：`src/App.tsx:144-155` - 在 `handleSelectConnection` 中检测占位符 ID，从 `displayConnections` 找到 `config_id` 并调用 `handleReconnect`。

### #75 Schema 树缺少创建表/视图/函数/存储过程的右键菜单 ✅ 已修复
- **现状**：Schema 树的数据库节点、表节点、视图节点等缺少"创建"相关的右键菜单项。用户只能手动编写 DDL 语句。
- **影响**：用户体验不友好，需要记忆 DDL 语法；无法快速创建新对象；缺少类似 Navicat/DBeaver 的常见功能。
- **期望功能**：
  - **数据库节点**右键菜单：创建表、创建视图、创建函数、创建存储过程
  - **表节点**右键菜单：创建表（新建空表）
  - **视图节点**右键菜单：创建视图
  - **函数节点**右键菜单：创建函数
  - **存储过程节点**右键菜单：创建存储过程
- **修复**：
  1. **数据库节点**右键菜单改为"查看表/视图/函数/存储过程"（展开对应分组）
  2. **表分组节点**右键菜单：添加"新建表"（可视化对话框）+ "创建表 (SQL)"
  3. **视图/函数/存储过程分组节点**：保持"创建..."菜单
  4. 菜单项点击后插入对应的 DDL 模板到编辑器（包含数据库名和占位符）
  5. 模板内容：
     - 表：`CREATE TABLE \`db\`.\`table_name\` (...)`
     - 视图：`CREATE VIEW \`db\`.\`view_name\` AS SELECT ...`
     - 函数：`DELIMITER $$ CREATE FUNCTION \`db\`.\`function_name\`() ...`
     - 存储过程：`DELIMITER $$ CREATE PROCEDURE \`db\`.\`procedure_name\`() ...`
  6. 文件改动：
     - `src/App.tsx:188-192` - 添加 `handleInsertTemplate` 函数
     - `src/App.tsx:622` - 传递 `onInsertTemplate` prop
     - `src/components/SchemaTree.tsx:27` - 添加 `onInsertTemplate` prop 类型
     - `src/components/SchemaTree.tsx:7-17` - 扩展 MenuNode 类型添加分组节点
     - `src/components/SchemaTree.tsx:80` - 添加 `onInsertTemplate` 参数
     - `src/components/SchemaTree.tsx:261/317/351/387` - 各分组节点添加 `onContextMenu` 处理
     - `src/components/SchemaTree.tsx:471-489` - 数据库节点改为"查看..."菜单
     - `src/components/SchemaTree.tsx:520-531` - 表分组节点添加"新建表"（对话框）+"创建表 (SQL)"
     - `src/components/SchemaTree.tsx:532-567` - 视图/函数/存储过程分组的"创建..."菜单和 DDL 模板生成逻辑
     - `src/locales/zh-CN.json` / `en-US.json` - 添加"查看表/视图/函数/存储过程"翻译
- **优先级**：P2 - 提升用户体验的重要功能
- **规模**：中型 - 需要修改 SchemaTree 右键菜单、添加多个 DDL 模板、处理编辑器插入逻辑

### #76 视图/函数/存储过程/触发器节点缺少"查看 DDL"菜单 ✅ 已修复
- **现状**：表节点有"查看 DDL"菜单，但视图/函数/存储过程/触发器节点没有，用户无法快速查看这些对象的创建语句。
- **影响**：用户体验不一致，需要手动编写 `SHOW CREATE VIEW/FUNCTION/PROCEDURE/TRIGGER` 语句。
- **方向**：
  1. 后端添加通用的 `show_create_object` 命令，支持 VIEW/FUNCTION/PROCEDURE/TRIGGER
  2. 前端在视图/函数/存储过程/触发器节点右键菜单中添加"查看 DDL"选项
  3. 点击后调用后端 API，将 DDL 插入到编辑器
- **修复**：
  - `src-tauri/src/commands.rs:340-372` - 添加 `show_create_object` 命令
  - `src-tauri/src/lib.rs:70` - 注册新命令
  - `src/api.ts:73-74` - 添加 `showCreateObject` API
  - `src/App.tsx:206-213` - 添加 `handleShowObjectDDL` 处理函数
  - `src/App.tsx:631` - 传递 `onShowObjectDDL` prop
  - `src/components/SchemaTree.tsx:29` - 添加 `onShowObjectDDL` prop 类型
  - `src/components/SchemaTree.tsx:81` - 添加 `onShowObjectDDL` 参数
  - `src/components/SchemaTree.tsx:510-527` - 视图/函数/存储过程/触发器节点添加"查看 DDL"菜单项
- **优先级**：P2 - 提升用户体验
- **规模**：小型 - 单个命令 + 前端菜单项添加

---

### #77 函数/存储过程 DDL 显示 sql_mode 而非 DDL ✅

- **现象**: 右键函数/存储过程节点，点击"查看 DDL"显示 STRICT_TRANS_TABLES,NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION 而非 DDL 语句
- **原因**: SHOW CREATE FUNCTION/PROCEDURE 返回 4 列（名称 + sql_mode + DDL + charset），而 show_create_object 命令错误地返回了第 2 列（sql_mode）而非第 3 列（DDL）
- **修复**:
  - src-tauri/src/commands.rs:340-372 - 根据对象类型选择正确的列索引（TABLE/VIEW 用第 2 列，FUNCTION/PROCEDURE/TRIGGER 用第 3 列）
- **优先级**: P1 - 功能不可用
- **规模**: 小型 - 单行逻辑修复

---

### #78 缺少执行存储过程的快捷方式 ✅

- **需求**: 在存储过程节点右键菜单中添加"执行"选项，快速生成 CALL 语句
- **实现**:
  - src-tauri/src/commands.rs:315-338 - 添加 execute_procedure 命令，生成 CALL procedure_name() 语句
  - src-tauri/src/lib.rs:71 - 注册新命令
  - src/api.ts:404-405 - 添加 executeProcedure API
  - src/App.tsx:194-202 - 添加 handleExecuteProcedure 处理函数
  - src/components/SchemaTree.tsx:31 - 添加 onExecuteProcedure prop
  - src/components/SchemaTree.tsx:528-531 - 存储过程节点添加"执行"菜单项
  - src/locales/*.json - 添加翻译
- **优先级**: P2 - 提升用户体验
- **规模**: 小型 - 单个命令 + 前端菜单项添加

---

### #79 DDL 模板缺少 DETERMINISTIC 特性 ✅

- **现象**: 创建函数 SQL 执行报错 ERROR 1418: This function has none of DETERMINISTIC, NO SQL, or READS SQL DATA
- **原因**: MySQL 开启二进制日志时，创建函数必须指定 DETERMINISTIC、NO SQL 或 READS SQL DATA 特性
- **修复**:
  - src/components/SchemaTree.tsx:564 - 函数 DDL 模板添加 DETERMINISTIC 关键字
- **优先级**: P1 - 模板不可用
- **规模**: 小型 - 单行模板修改

---

### #80 视图 DDL 显示未格式化的单行 SQL ✅

- **现象**: 右键视图节点点击"查看 DDL"，显示的 SQL 是未格式化的单行文本，难以阅读
- **原因**: SHOW CREATE VIEW 返回的 DDL 是压缩的单行文本，没有换行和缩进
- **修复**:
  - src-tauri/src/commands.rs:1259-1289 - 添加 ormat_sql 辅助函数，在主要关键字（SELECT/FROM/WHERE/JOIN 等）前添加换行
  - src-tauri/src/commands.rs:1326-1330 - 在返回 DDL 前调用 ormat_sql 进行格式化
- **优先级**: P2 - 改善用户体验
- **规模**: 小型 - 单个格式化函数
