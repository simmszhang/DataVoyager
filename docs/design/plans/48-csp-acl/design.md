# #48 CSP + ACL 门控 — 设计文档

> 状态：评审需重写（2 阻断项已修订，待复审） · 优先级 P0 · 规模：大 · 关联缺陷：#12（危险分析误报）、#51（warn 级）、#22（脱敏联动）· 依赖共享契约：S6（CSP 底线）、S3（脱敏）、S5（错误形状）

## 1. 现状与影响

- `tauri.conf.json:22-24`：`"security": { "csp": null }`，CSP 关闭。
- `capabilities/default.json:6-8`：仅 `"core:default"`，自定义命令对 WebView 全量开放（review S15）。
- `danger.rs:29-59`：`analyze_danger` 纯关键词匹配，字符串字面量里的 `DROP`/`DELETE` 误判（`danger.rs:99-100` 测试自证 `SELECT 'drop'` 被标危险）；`Warn` 变体从未构造（`danger.rs:10-12`）。
- `App.tsx:190-194`：前端只判 `dangerous`，`warn` 直接放行（#51）。
- `commands.rs:288-327,450-499`：`execute_query`/`execute_query_stream` 接受任意 SQL，**无任何服务端危险复检**——「手打 DROP TABLE」与 XSS 最可能调用此路径。
- `list_saved_connections` 泄 ssh.password（#22，S3）。
- **影响**：一旦 XSS（CSP 关闭放大），攻击面覆盖全部破坏性命令 + 凭据读取；危险确认可被字符串误报干扰；任意 SQL 路径无纵深防御。

## 2. 目标与成功标准

1. 启用显式 CSP（S6），收紧脚本/连接源。
2. 破坏性命令（含任意 SQL 执行）加**服务端危险复检 + `confirmed`**，覆盖 `execute_query`/`execute_query_stream`（手打 SQL 路径）与 schema 命令。
3. `analyze_danger` 升级为轻量 tokenizer，跳过字符串/注释内关键词（#12）。
4. `Warn` 级落地并被前端正确处置（#51），且降级不削弱破坏性 SQL 的阻断。
5. `list_saved_connections` 脱敏（由 #22 交付，本方案验证）。
6. 成功标准：`execute_query_stream("DROP TABLE t", confirmed=false)` 被服务端拒绝；`confirmed=true` 放行；`SELECT 'drop'` 不再误报；`UPDATE 无 WHERE` 走 Warn 仍提示；`DELETE 无 WHERE` 仍 Dangerous。

## 3. 威胁模型（诚实界定）

- **CSP**（S6）是**脚本注入**的第一道防线：阻止 XSS 代码被加载/执行。
- **服务端危险复检 + `confirmed`** 是**纵深防御**：拦截「绕过前端确认的不完整流程/逻辑 bug」。`confirmed` 参数对**已获脚本执行权的蓄意 XSS 可伪造**（XSS 可 `invoke(..., confirmed:true)`），故它**不是**对 XSS 的硬边界。
- **残余风险**：Tauri 桌面应用中，WebView 对全部自定义命令天然具备 `invoke` 能力；一旦 XSS 成立且可伪造 `confirmed`，破坏性命令仍可被调用。真正阻断该残余风险需「原生确认对话框（Tauri dialog 插件，WebView JS 无法驱动/伪造）」或「按命令细分的 capability 授权」——列为后续加固（M2），本方案不承诺彻底阻断蓄意 XSS。
- **结论**：本方案组合「CSP + 服务端复检 + 脱敏」，把「意外/未确认调用」与「凭据读取」压到最低；对「蓄意 XSS 直接 invoke」只做缓解、不承诺绝对阻断。

## 4. 推荐方案详细设计

### 4.1 CSP（`src-tauri/tauri.conf.json`）

```json
"security": {
  "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost"
}
```

- `script-src 'self'`：生产构建（Vite）无 eval/内联脚本；CodeMirror 6 不需要 `unsafe-eval`。
- `style-src 'self' 'unsafe-inline'`：CodeMirror 动态内联样式需要。
- `connect-src ipc: http://ipc.localhost`：Tauri 2 IPC 通道，**以所依赖 Tauri 2 版本的 IPC origin 为准，需 dev 实测钉版本**。
- **dev 模式差异**：`pnpm tauri dev` 下 Vite 的 react-refresh/HMR 依赖内联脚本（可能需 `script-src 'unsafe-inline'`）；故 dev 与 prod 用**不同的 CSP**——Tauri 2 单值配置无法区分时，采用「prod 严格 CSP 写入 `tauri.conf.json`；dev 用 `beforeDevCommand` 环境或临时放开 `script-src`」的折中，并显式记录 dev/prod 差异，避免生产误放宽。

### 4.2 服务端危险复检（`src-tauri/src/commands.rs`）

```rust
fn guard_dangerous(sql: &str, confirmed: bool) -> Result<()> {
    if dby_core::danger::analyze_danger(sql).is_dangerous() && !confirmed {
        return Err(DbError::Config("危险操作需二次确认".to_string()));
    }
    Ok(())
}
```

**门控范围**（补全任意 SQL 路径，修正 no-op）：

| 命令 | 复检对象 | 说明 |
| --- | --- | --- |
| `execute_query(id, db, sql, confirmed)` | `sql` | **任意 SQL**：手打 DROP/DELETE 无 WHERE 等在此拦截 |
| `execute_query_stream(channel, id, db, sql, confirmed)` | `sql` | 流式同上 |
| `export_result(id, db, sql, format, table, confirmed)` | `sql` | **导出重跑任意 SQL**（`commands.rs:759-824` 服务端 `execute_buffered` 重跑 `sql`），同样拦截 |
| `drop_database(id, name, confirmed)` | 生成的 DROP SQL | schema 命令 |
| `drop_table(id, db, name, confirmed)` | 生成的 DROP SQL | schema 命令 |
| `rename_table(..., confirmed)` | 生成的 ALTER/RENAME SQL | schema 命令 |
| `delete_project(id, confirmed)` | —（非 SQL） | 直接要求 `confirmed`，与 UI 确认一致 |
| ~~`execute_edit`~~ | — | **移出**：`build_update` 恒带 WHERE 主键、按语义恒 Safe，danger 复检对其是 no-op；其门控是「编辑来源校验」(#26)，不在本方案 |

前端 `handleRun`/`confirmEdit`/`confirmDanger` 在确认通过后传 `confirmed=true`；其余路径传 `false`。

### 4.3 危险分析 tokenizer（#12）

`danger.rs` 内新增状态机（与 `split_statements` 同思路）：在单引号/双引号/反引号/行注释/块注释内**跳过**关键词匹配，其余按词边界匹配关键词。

> **关键词清单需扩展**：现 `danger.rs:35-48` 仅有 `DROP/TRUNCATE/ALTER/DELETE/UPDATE`；须**新增 `RENAME`**，否则 `rename_table`（生成 `RENAME TABLE`，`ddl.rs:48-54`）的门控恒为 Safe、重蹈 execute_edit 的 no-op。**既有测试反转**：`danger.rs:99-100` 现在自证 `SELECT 'drop'` 被判危险，tokenizer 落地后该断言须反转为 Safe。

### 4.4 分级语义（#51/#57，修正降级安全）

- **Dangerous**：`DROP`/`TRUNCATE`/`ALTER`/`RENAME` + **`DELETE` 无 WHERE**（不可逆数据删除，保持最高阻断）。
- **Warn**：**`UPDATE` 无 WHERE**（大范围修改，可回滚/有备份可恢复）。
- **Safe**：其余（含带 WHERE 的 DELETE/UPDATE、SELECT、DDL 外语句）。
- 前端：`dangerous` → 阻断式确认弹窗；`warn` → 轻量提示（带「仍要执行」确认），**不静默放行**。

### 4.5 凭据读取收敛（#22，S3）

`list_saved_connections` 返回脱敏视图（由 #22 交付），本方案回归验证「返回体不含 secret」。

## 5. 错误处理（遵循 S5）

- 危险未确认：`DbError::Config("危险操作需二次确认")`。**注**：理想是新增专用 kind `DangerDenied`（扩展 S5 的 8 变体为 9），本方案先以 `Config` + 明确 message 落地，`DangerDenied` 随 #29 错误模型统一时补上。
- CSP 违规：由 WebView 控制台报错，需在 dev 验证阶段清零。

## 6. 测试策略

- **单元（danger.rs）**：字符串/注释内 `DROP/DELETE` 不再命中；真实 `DROP` 仍 Dangerous；`DELETE 无 WHERE` → Dangerous、`UPDATE 无 WHERE` → Warn、带 WHERE → Safe；多语句收集多 reason。
- **壳层**：`guard_dangerous` 两路（dangerous+未确认=Err、confirmed=true=Ok、safe=Ok）；`execute_query`/`execute_query_stream` 对危险 SQL 未确认拒绝。
- **手工/集成**：`pnpm tauri dev` 验证 CSP 下连接/查询/流式/导出全链路无报错，并钉定 IPC origin 版本；注入片段（`<img onerror=...>`）被 CSP 阻断。

## 7. 回归风险与影响面

- CSP 收紧可能误伤 CodeMirror 内联样式、Tauri IPC、dev 模式 react-refresh/HMR → 需 dev 实测，dev/prod 分开处理。
- 任意 SQL 命令加 `confirmed`：前端 `api.executeQuery/executeQueryStream` + 所有调用点同步，未确认的危险 SQL 会被服务端拒绝（行为收紧）。
- `analyze_danger` 语义变化：`UPDATE 无 WHERE` 从 Dangerous 降为 Warn → 前端必须正确处置 Warn（#51 正是此坑）；`DELETE 无 WHERE` 保持 Dangerous 不降。
- `execute_edit` 移出 danger 门控后，其破坏性由 #26（编辑来源校验）兜底，需在 #26 方案落地前不削弱。

## 8. 关联缺陷处置

- #12：4.3 tokenizer；#51/#57：4.4 分级；#22：4.5 脱敏验证；#48 核心：4.1/4.2 门控。

## 9. 与其它方案组的依赖

- 依赖 #22（S3 脱敏）；依赖 S5（错误形状，含后续 `DangerDenied` kind 扩展）；与 #5（S1 并发）无耦合，但 `guard_dangerous` 调用点同样遵循「锁不跨 await」；后续「原生确认对话框」加固依赖 Tauri dialog 插件（M2）。
