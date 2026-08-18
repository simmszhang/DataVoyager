# 各子系统设计

每个子系统：目标 → 方案 → 边界与取舍（含指向 defects 的引用）。

> 首轮逐层评审后新增了横切关注点（类型映射层/错误模型/并发模型/安全模型），见 [`architecture.md`](./architecture.md) §8–13；逐项证据见 [`review.md`](./review.md)。新增缺陷编号为 #21 起。

## 1. 驱动异步化（mysql_async）

- **目标**：流式 + 可取消的 MySQL 查询。
- **方案**：同步 `mysql` 换 `mysql_async`；`Connection` trait 用 `async_trait` 异步化；`tokio::sync::Mutex` 替代 `std::sync::Mutex`。
- **取舍**：命令层从 `spawn_blocking` 样板简化为原生 `async fn`；代价是 trait 对象 + 借用更受约束（用 `ResultSink` 回调规避返回借用流）。

## 2. 流式 + 取消

- **方案**：`StreamEvent`（Columns/Rows/Affected）+ `ResultSink` 回调；Tauri `Channel` 推流；`CancellationToken` 批间检查。
- **取舍**：
  - 取消是 **drain 式**（返回 `Cancelled` 后 drop 结果流会排空剩余行）→ defects #5。
  - 评审新增：**全局锁跨 `await`** 使 `cancel_query` 抢不到锁、取消实际失效 → #21；**令牌不 reset**（sticky）→ #23；**DML 不可取消** → #34；**流式无终止事件** → #42。

## 3. 多连接标签页工作区

- **方案**：Zustand `workspaces: Record<connId, ...>` 每连接独立状态；`tabs` + 连接标签栏；`mutateResult` 就地追加。
- **取舍**：关闭标签 = 断开连接（无「仅关标签保留连接」）→ defects #7；切项目 `setActive(null)` 丢活动连接 → defects #16；评审新增 tabs 不按项目过滤（跨项目残留）→ #54、status 无 per-connection 语义 → #67。

## 4. 项目 CRUD

- **方案**：项目存 JSON 配置；连接强制挂 `project_id`；删除项目前校验无连接。
- **取舍**：重命名/删除用 `window.prompt/confirm` → defects #2；评审新增 `delete_project` 只校验活跃连接、遗留孤儿配置与钥匙串 → #41。

## 5. 凭据 keyring + 连接持久化

- **方案**：密码进 OS 钥匙串（`keyring`，key=连接 uuid）；磁盘 JSON 无明文；`reconnect` 读配置 + 钥匙串密码。
- **取舍**：每次 connect 都自动持久化，无「保存连接」开关 → defects #6；评审新增 **SSH 密码/私钥明文落盘 + 经 IPC 返回前端** → #22（🔴）、写错误被吞 → #39、无 update 命令/`color` 死字段 → #63。

## 6. 虚拟滚动网格

- **方案**：`@tanstack/react-virtual` 行虚拟化；固定行高 30px、列宽 200px；sticky 表头。
- **取舍**：列宽固定、无自动列宽/列宽调整 → defects #10；评审新增 `resultVersion` 死字段 → #64、`mutateResult` 就地 mutate → #65。

## 7. 事务 + autocommit

- **方案**：`begin/commit/rollback/set_autocommit` 命令；事务态在 workspace 内（每连接独立）；状态栏「● 事务中」。
- **取舍**：事务内未显式禁用流式；autocommit 无持久化；评审新增 `begin` 嵌套隐式提交 + 无连接侧状态 → #37。

## 8. 数据编辑

- **方案**：双击单元格 → `build_edit_sql`（方言感知 UPDATE）→ 预览确认 → `execute_edit`（origin=data_edit）→ 重跑刷新。
- **取舍**：值类型在前端用正则解析（`toCellValue`）不感知列类型 → defects #11；评审新增编辑来源未校验（手写 JOIN 后误改）→ #26、模态框不绑定连接 → #25、编辑类型覆盖不足 → #69。

## 9. 导出

- **方案**：`dby-core::export`（CSV/JSON/Markdown/INSERT）；`export_result` 命令重跑查询 + 格式化；前端弹窗复制到剪贴板。
- **取舍**：**无大结果集完整导出**。评审修正：导出**不截断、全量收集进内存、整串过 IPC**（`ExecOpts::default()`，非文档原称的「2000 行截断」）→ #38；format 校验前就执行并记历史 → #46。

## 10. 历史面板

- **方案**：SQLite FTS5；语句库（去重）+ 执行流水（append-only）两视图；搜索/固定/删除执行/载入编辑器。
- **取舍**：语句库删除未实现 → defects #14；历史写入同步 → defects #13；评审新增 `clear` 不清理 FTS + 跨项目归因有损 → #31、搜索无防抖 → #52、按字面量去重 → #56。

## 11. 危险操作确认

- **方案**：`dby-core::danger::analyze_danger`（逐句 + 关键词）；前端执行前分析 + 确认弹窗。
- **取舍**：纯关键词匹配，字符串字面量里 `DROP` 误报 → defects #12；评审新增 `warn` 级被前端忽略 → #51、`Warn` 死变体 → #57。

## 12. SSH 隧道

- **方案**：`russh` 本地端口转发（SSH direct-tcpip → 本地临时端口 → 驱动连 127.0.0.1:端口）。
- **取舍**：仅密码认证 → defects #8；接受任意主机密钥 → defects #9；评审新增 **断开后资源泄漏**（accept 循环不退出）→ #24（🔴）、无连接超时 + 失败被吞 → #36、SSH 密码明文落盘 → #22。

## 13. SSL/TLS

- **方案**：`mysql_async` + `rustls` + `ring`；`SslOptions.enabled/verify_cert`。
- **取舍**：`verify_cert=false` 接受任意证书；`ca_path/client_cert/client_key` 已建模未接线（CA/双向证书 M2）。

## 14. Schema 树 + 右键菜单 + CRUD

- **方案**：`SchemaTree` 懒加载树（连接→库→表→列）+ 右键菜单 + `ddl.rs` 生成 DDL + 5 个 DDL 命令。
- **取舍**：树节点 key 用 `:` 拼接再 split → defects #3；建库不在右键菜单；删除/重命名用 prompt/confirm → defects #2；评审新增展开无「已加载」短路、缓存无失效 → #66。

## 15. 测试策略（评审新增）

- **单元测试**：`dby-core`（value/query/dialect/history/edit/ddl/export/danger 均有单测）+ `dby-driver-mysql`（conv.rs 单测）。
- **集成测试**：`mysql_integration.rs`（`#[ignore]`，需真实 MySQL），覆盖 connect/ping/schemas、缓冲路径 DDL/DML/SELECT、事务回滚/提交、流式 + 取消后复用。
- **覆盖缺口**（详见 `review.md` §六）：SSH/TLS **完全无测试**；`columns/indexes/foreign_keys/triggers/procedures/table_ddl/catalogs` 未断言；取消仅验证 drain 未验证「秒断」；`Time`/DECIMAL/JSON/TINYINT(1)/大结果集（>100 行分批）/多结果集/错误路径/`last_insert_id`/`set_autocommit` 未测；`tokio::time` 依赖传递启用（#62）。
- **门禁**：CI 跑 `cargo fmt --check` + `clippy -D warnings` + 纯 crate 测试 + `pnpm build`；集成测试独立 job（MySQL service）。

## 16. 评审新增的横切子系统

以下关注点在首轮评审中暴露，已并入 `architecture.md`：**类型映射层**（§8，支撑 #1/#11/#20/#32/#33）、**错误模型**（§9，支撑 #19/#29/#39）、**多结果集语义**（§10，支撑 #28）、**并发模型**（§11，支撑 #21/#23）、**安全模型**（§12，支撑 #9/#22/#24/#48）、**IPC 契约**（§13，支撑 #42/#45/#47）。
