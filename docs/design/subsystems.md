# 各子系统设计

每个子系统：目标 → 方案 → 边界与取舍（含指向 defects 的引用）。

## 1. 驱动异步化（mysql_async）

- **目标**：流式 + 可取消的 MySQL 查询。
- **方案**：同步 `mysql` 换 `mysql_async`；`Connection` trait 用 `async_trait` 异步化；`tokio::sync::Mutex` 替代 `std::sync::Mutex`。
- **取舍**：命令层从 `spawn_blocking` 样板简化为原生 `async fn`；代价是 trait 对象 + 借用更受约束（用 `ResultSink` 回调规避返回借用流）。

## 2. 流式 + 取消

- **方案**：`StreamEvent`（Columns/Rows/Affected）+ `ResultSink` 回调；Tauri `Channel` 推流；`CancellationToken` 批间检查。
- **取舍**：取消是 **drain 式**（返回 `Cancelled` 后 drop 结果流会排空剩余行，慢查询排空久）→ defects #5。

## 3. 多连接标签页工作区

- **方案**：Zustand `workspaces: Record<connId, ...>` 每连接独立状态；`tabs` + 连接标签栏；`mutateResult` 就地追加。
- **取舍**：关闭标签 = 断开连接（无「仅关标签保留连接」）→ defects #7；切项目会 `setActive(null)` 丢当前活动连接 → defects #16。

## 4. 项目 CRUD

- **方案**：项目存 JSON 配置；连接强制挂 `project_id`；删除项目前校验无连接。
- **取舍**：重命名/删除用 `window.prompt/confirm`（与生产级 UI 不符）→ defects #2。

## 5. 凭据 keyring + 连接持久化

- **方案**：密码进 OS 钥匙串（`keyring`，key=连接 uuid）；磁盘 JSON 无明文；`reconnect` 读配置 + 钥匙串密码。
- **取舍**：**每次 connect 都自动持久化**，无「保存连接」开关 → defects #6。

## 6. 虚拟滚动网格

- **方案**：`@tanstack/react-virtual` 行虚拟化；固定行高 30px、列宽 200px；sticky 表头。
- **取舍**：**列宽固定、无自动列宽/列宽调整** → defects #10。

## 7. 事务 + autocommit

- **方案**：`begin/commit/rollback/set_autocommit` 命令；事务态在 workspace 内（每连接独立）；状态栏「● 事务中」。
- **取舍**：事务内未显式禁用流式（大结果集会锁连接）；autocommit 无持久化。

## 8. 数据编辑

- **方案**：双击单元格 → `build_edit_sql`（方言感知 UPDATE）→ 预览确认 → `execute_edit`（origin=data_edit）→ 重跑刷新。
- **取舍**：**值类型在前端用正则解析**（`toCellValue`），不感知列类型（数字列输字符串靠 MySQL 隐式转换）→ defects #11。

## 9. 导出

- **方案**：`dby-core::export`（CSV/JSON/Markdown/INSERT）；`export_result` 命令重跑查询 + 格式化；前端弹窗复制到剪贴板。
- **取舍**：导出走缓冲路径（受 2000 行 `max_rows` 截断），**无大结果集完整导出**。

## 10. 历史面板

- **方案**：SQLite FTS5；语句库（去重）+ 执行流水（append-only）两视图；搜索/固定/删除执行/载入编辑器。
- **取舍**：语句库删除（`delete_statement`）未实现（FTS 同步问题）→ defects #14；历史写入同步 → defects #13。

## 11. 危险操作确认

- **方案**：`dby-core::danger::analyze_danger`（逐句 + 关键词：DROP/TRUNCATE/ALTER、无 WHERE 的 DELETE/UPDATE）；前端执行前分析 + 确认弹窗。
- **取舍**：**纯正则/关键词匹配**，字符串字面量里的 `DROP` 等会误报 → defects #12。

## 12. SSH 隧道

- **方案**：`russh` 本地端口转发（SSH direct-tcpip → 本地临时端口 → 驱动连 127.0.0.1:端口）。
- **取舍**：**仅密码认证**（私钥未实现）→ defects #8；**接受任意主机密钥**（无指纹确认）→ defects #9（安全）。

## 13. SSL/TLS

- **方案**：`mysql_async` + `rustls` + `ring`；`SslOptions.enabled/verify_cert`。
- **取舍**：`verify_cert=false` 时接受任意证书；CA 校验/双向证书未实现。

## 14. Schema 树 + 右键菜单 + CRUD

- **方案**：`SchemaTree` 懒加载树（连接→库→表→列）+ 右键菜单 + `ddl.rs` 生成 DDL + 5 个 DDL 命令。
- **取舍**：树节点 key 用 `:` 拼接再 split 解析（脆弱）→ defects #3；建库不在右键菜单（只有建表）；删除/重命名用 prompt/confirm → defects #2。
