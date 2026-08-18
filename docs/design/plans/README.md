# 缺陷修复方案索引（档位 C · 需统一方案）

> 本目录是「定方案 → 评审 → 实现」的**方案文档**所在地。每个方案组一份 `design.md`（设计文档）+ 一份 `plan.md`（分步实现计划）。
>
> 评审门禁：`design.md` 评审通过后，`plan.md` 才可进入实现阶段（`subagent-driven-development` 或 `executing-plans`）。

## 文件布局

| 方案组 | 目录 | P | 规模 | 本组捆绑的关联缺陷 |
| --- | --- | --- | --- | --- |
| #9 SSH 主机密钥 TOFU | [`09-ssh-host-key-tofu/`](./09-ssh-host-key-tofu/) | P0 | 大 | #8 私钥认证、#36 SSH 超时/诊断 |
| #22 SSH 凭据存储 | [`22-ssh-credential-storage/`](./22-ssh-credential-storage/) | P0 | 大 | #6 保存开关、#39 写错误、#41 级联清理、#63 update 命令 |
| #48 CSP + ACL 门控 | [`48-csp-acl/`](./48-csp-acl/) | P0 | 大 | #22 脱敏联动、#12/#51 危险确认 |
| #1 Value 类型映射 | [`01-value-type-mapping/`](./01-value-type-mapping/) | P1 | 大 | #20/#32/#33/#60/#11/#69、#45 联动 |
| #4 方言感知 SQL 生成 | [`04-dialect-aware-sql/`](./04-dialect-aware-sql/) | P1 | 大 | #3 节点 key、#59 split 并入 Dialect、#61 元数据拼接 |
| #5 取消秒断 | [`05-cancel-sec-break/`](./05-cancel-sec-break/) | P1 | 大 | #21 全局锁、#23 sticky cancel、#34 DML 取消 |
| #24 SSH 隧道生命周期 | [`24-ssh-tunnel-lifecycle/`](./24-ssh-tunnel-lifecycle/) | P1 | 大 | #36 |
| #28 多结果集协议 | [`28-multi-resultset/`](./28-multi-resultset/) | P1 | 大 | #42 流终止事件 |
| #45 数值精度 | [`45-numeric-precision/`](./45-numeric-precision/) | P1 | 大 | #1 联动 |
| #18 i18n | [`18-i18n/`](./18-i18n/) | P3 | 大 | 全前端文案 key 化 |

每个目录内含：

- `design.md` —— 设计文档（现状/影响 → 目标 → 方案对比 → 推荐方案详设 → 错误处理 → 测试 → 回归风险 → 关联缺陷）
- `plan.md` —— 实现计划（依据 design.md，bite-sized 任务，含代码与验证命令）

---

## 跨方案共享契约（所有 design.md/plan.md 必须与此一致）

以下目标设计已由首轮评审确认（见 `../architecture.md` §8–13），是多个方案组的共同地基。任何方案组不得与之冲突；需要调整时，改本文件并同步受影响方案组。

### S1 并发模型（#5 的地基，其余各组涉及 commands 改动时遵循）

- **现状**：`state.rs:26` 是单把全局锁 `connections: Mutex<HashMap<u64, ActiveConnection>>`，所有命令在 `guard` 存活期间 `await` 网络 I/O（`commands.rs` 多处）。
- **目标**：连接注册表 + per-connection 锁：

  ```rust
  // state.rs
  pub struct AppState {
      // 外层注册表：std::sync::Mutex，只做 get/clone/insert/remove，绝不跨 await 持有
      pub connections: std::sync::Mutex<HashMap<u64, Arc<futures::lock::Mutex<ActiveConnection>>>>,
      pub query_tokens: std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>, // 查询实例级取消注册表
      // ...其余字段不变
  }
  ```

  > **为何 per-connection 用 `futures::lock::Mutex`**：`tokio::sync::MutexGuard` 非 `Send`，跨 `.await` 持锁会使命令 future 变 `!Send`，Tauri 2 `#[tauri::command]`（经 `tokio::spawn`）要求 `Future + Send`，会编译失败；`futures::lock::MutexGuard` 是 `Send`，可安全跨 await。外层注册表不跨 await，用 `std::sync::Mutex`。

- 命令取连接范式（外层锁只做同步 get，per-connection 锁可跨 await）：

  ```rust
  let entry = state.connections.lock().unwrap().get(&id).cloned() // std::sync::Mutex 同步锁，只借 Arc
      .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
  let mut active = entry.lock().await; // futures::lock::Mutex：Send guard，可跨 await
  ensure_connected(&state, &mut active).await?; // 毒化重连
  active.conn.execute_stream(...).await
  ```

- **取消**：`CancellationToken` **按查询实例创建**（每次 `execute_*` 新建 token，登记 `"{conn_id}:{query_id}" → token`，用 Drop guard 保证全路径注销）；`cancel_query(conn_id)` 只读该连接的 token 集合的 `Arc`，**不抢连接锁**。令牌按实例创建即天然修复 sticky cancel（#23），无需 `reset()`。`CancellationToken` 用 `tokio::sync::watch` 存取消态（无 `Notify` 丢失唤醒）。

### S2 类型映射层（#1 的地基，#45/#4/#11/#69 依赖）

- 新增结构化列类型（放在 `dby-core`，建议 `metadata.rs` 或新 `types.rs`）：

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  pub enum ColumnTypeBase {
      Bool, I8, I16, I32, I64, U8, U16, U32, U64, F32, F64,
      Decimal, Str, Bytes, Date, Time, DateTime, Json, Uuid, Array, Map, Unknown,
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct ColumnType {
      pub base: ColumnTypeBase,
      pub numeric_precision: Option<u32>,
      pub numeric_scale: Option<u32>,
      pub unsigned: bool,
      pub char_max_length: Option<u32>,
      pub temporal_precision: Option<u32>, // datetime(6)/timestamp(6) 小数秒位
      pub charset: Option<String>,
      pub collation: Option<String>,
  }
  ```

- `ColumnInfo` 增加 `column_type: Option<ColumnType>`（**保留** `type_name` 作为展示名，避免一次性破坏前端契约；`type_name` 未来统一由 `Dialect::display_type_name(&ColumnType)` 生成）。
- `Dialect` trait 增加：`fn parse_column_type(&self, raw: &str) -> Option<ColumnType>` 与 `fn display_type_name(&self, ct: &ColumnType) -> String`（替代现有 `display_type_name(raw: &str)` 的 Debug 字符串脆弱实现，消除 D3 两条路径不一致）。
- 驱动值转换改为**列类型驱动**：`fn convert(v: &MyValue, ct: &ColumnType) -> Value`，规则见 `architecture.md` §8（BLOB/BINARY→Bytes、JSON→Json、DECIMAL→Decimal、`TINYINT(1)`→可选 Bool、Date/Time/DateTime 按列类型而非时分秒推断）。

### S3 凭据存储与脱敏（#22 的地基，#48/#9 引用）

- `SshOptions.password` / `private_key` 加 `#[serde(skip_serializing)]`（**保留反序列化**，供 `connect` 入参）。
- 钥匙串 key 约定（`keyring::Entry::new("dby", key)`）：
  - MySQL 密码：`"{config_id}"`（现状已如此）
  - SSH 密码：`"{config_id}:ssh"`
  - SSH 私钥：`"{config_id}:ssh_key"`
- `list_saved_connections` 返回**脱敏视图**（不含任何 secret；含 `has_ssh: bool`、`ssh_host/ssh_user` 等元数据），前端 `SavedConnection` 类型同步收窄。
- `reconnect` 从钥匙串读取 secrets；`delete_saved_connection` / `delete_project` 级联删除对应钥匙串条目。

### S4 流式协议（#28 的地基，#42/#5 引用）

- `StreamEvent` 增加变体（`query.rs`）：

  ```rust
  pub enum StreamEvent {
      Columns(Vec<ColumnInfo>),
      Rows(Vec<Vec<Value>>),
      Affected { affected_rows: u64, last_insert_id: Option<u64> },
      Info(Option<String>),
      ResultSetEnd,          // 新增：结果集边界
      Truncated,             // 新增：超 max_rows 截断
      Done,                  // 新增：命令成功收尾（channel 上的终止事件）
      Error { message: String }, // 新增：命令失败收尾
  }
  ```

- `CollectingSink` 按 `ResultSetEnd` 分桶，`QueryOutput.result_sets` 真正多组；驱动 `query_iter` 遍历 `next_result_set()` 全部结果集。
- `ChannelSink` 在 `execute_query_stream` 末尾发 `Done`/`Error`，`channel.send` 失败时主动触发取消（#42）。

### S5 错误形状（#19/#29 的地基，各方案错误处理引用）

- `DbError` 的 `Serialize` 升级为 `{"kind": "...", "message": "..."}`（`kind` 取 8 变体小写蛇形：`database/driver_not_found/connection_not_found/unsupported/config/storage/cancelled/other`）。
- 前端新增 `ApiError` 类型 + `errToString()` 统一解析（替代 `String(e)`，修复 `[object Object]`）。

### S6 CSP 底线（#48 独有，其余方案不得降低）

- `tauri.conf.json` 的 `"csp": null` 必须替换为显式策略。最终精确 policy 由 #48 方案组定（需兼容 CodeMirror 内联样式 + Tauri IPC 的 `connect-src`），评审通过后写死。

---

## design.md 模板

```markdown
# [编号] [标题] — 设计文档

> 状态：待评审 · 优先级 Px · 规模：大 · 关联缺陷：#a/#b · 依赖：#c（可选）

## 1. 现状与影响
（现状代码 + `file:line` 证据；可复现的影响描述）

## 2. 目标与成功标准
（可验收的明确标准，逐条列出）

## 3. 方案对比
（2–3 个方案：思路 + 取舍 + 推荐；明确推荐理由）

## 4. 推荐方案详细设计
（组件划分 / 精确接口签名 / 数据流，落到具体文件与类型；引用共享契约 Sx）

## 5. 错误处理
（失败路径与错误形状，遵循 S5）

## 6. 测试策略
（单元 + 集成；引用当前覆盖缺口与新增用例）

## 7. 回归风险与影响面
（哪些现有行为/契约会变；前端/驱动/壳层影响面）

## 8. 关联缺陷处置
（本组捆绑的每个关联缺陷如何被本方案覆盖）

## 9. 与其它方案组的依赖
（依赖顺序 / 共享契约引用）
```

## plan.md 模板

```markdown
# [编号] [标题] — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** [一句话目标]

**Architecture:** [2–3 句方案概述]

**Tech Stack:** [关键依赖/语言]

**Spec:** `docs/design/plans/<目录>/design.md`（本计划依据的设计文档）

## Global Constraints
[从 design.md + 共享契约提取的项目级约束，逐条列]

---
### Task N: [组件名]
**Files:**
- Create: `exact/path`
- Modify: `exact/path:行号`
- Test: `exact/path`

**Interfaces:**
- Consumes: [依赖前置任务的精确签名]
- Produces: [后续任务依赖的精确签名]

- [ ] **Step 1: 写失败测试**（代码块）
- [ ] **Step 2: 运行确认失败**（命令 + 预期）
- [ ] **Step 3: 最小实现**（代码块）
- [ ] **Step 4: 运行确认通过**（命令 + 预期）
- [ ] **Step 5: Commit**（命令）
```

> 计划必须「无占位符」：每个 Step 含真实代码/命令，不写 TODO/TBD/「添加适当错误处理」之类。

---

## 建议评审顺序

1. **第一梯队（P0 安全 + 核心架构）**：#9、#22、#48、#1、#5 —— 先评，彼此有耦合（取消语义 S1、凭据/脱敏 S3）。
2. **第二梯队**：#24、#28、#45、#4 —— 依赖第一梯队定的共享契约（S1/S2/S4）。
3. **第三梯队**：#18 —— 独立，可随时并行。

依赖关系：#1（S2）→ #45；#5（S1）→ 其余涉及 commands 的方案；#28（S4）→ #42；#22（S3）→ #48 脱敏。
