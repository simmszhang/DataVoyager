# #28 多结果集协议 — 设计文档

> 状态：评审需修订（1 阻断项已修订，待复审） · 优先级 P1 · 规模：大 · 关联缺陷：#42（流终止事件）· 依赖共享契约：S4（流式协议）、S5（错误形状）、S1（#5 的查询实例 token）

## 1. 现状与影响

- `query.rs:91-96`：`StreamEvent` 只有 `Columns/Rows/Affected/Info`，无「结果集结束」标记。
- `query.rs:162-183`：`CollectingSink` 对每个 `Columns` 仅**覆盖** `self.columns`，所有 `Rows` 追加到**同一** `rows`；`into_output`（`143-158`）永远只产一个 `ResultSet`。
- `lib.rs:263-300`：驱动 `query_iter` 只读首个结果集（`QueryResult` 无 `next_result_set()`，公开面仅 `columns()/next()/is_empty()/affected_rows()/last_insert_id()/drop_result()`）。
- `commands.rs:444`：`ChannelSink.on_event` 忽略 `channel.send` 结果；命令终态走 `invoke` 返回值，与 channel 是两条传输，收尾竞态；前端关标签后后端仍执行成无主孤儿（#42，review S8）。
- **影响**：存储过程 `CALL`、多语句的后续结果集被静默丢弃或错误合并；前端无法可靠判断「最后一批」与「命令返回」顺序；关闭标签后慢查询继续空转。

## 2. 目标与成功标准

1. `StreamEvent` 增加 `ResultSetEnd/Truncated/Done/Error`（S4）。
2. `CollectingSink` 按结果集边界分桶，`QueryOutput.result_sets` 真正多组。
3. 驱动用 `columns()+next()+is_empty()` 遍历全部结果集（mysql_async 0.37 真实 API）。
4. `ChannelSink` 在命令末尾发 `Done`/`Error`（携带 kind）；`channel.send` 失败时主动触发取消（#42）。
5. 前端多结果集可展示/切换；`running` 由 `done`/`error` 复位，与 `finally` 不冲突。
6. 成功标准：`CALL proc` 返回 2 结果集 → `result_sets.len()==2` 且各行归位；前端收到 `done` 才复位 `running`；关闭标签后后端查询被取消。

## 3. 方案对比

### 方案 A：`ResultSetEnd` 边界事件 + `Done/Error` 终止事件（推荐）
- 显式边界 + 终止事件，sink 分桶，channel 发终止。
- **优点**：语义清晰、流式与缓冲统一、收尾无竞态。**缺点**：`StreamEvent` 变体扩展。

### 方案 B：`Columns` 带结果集序号
- **缺点**：序号变化=边界的约定绕；终止态仍缺，否决。

### 方案 C：仅修缓冲路径
- **缺点**：#42 收尾竞态未解决，流式多结果集无法表达，否决。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 `StreamEvent` 扩展（`crates/dby-core/src/query.rs`，S4）

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum StreamEvent {
    Columns(Vec<ColumnInfo>),
    Rows(Vec<Vec<Value>>),
    Affected { affected_rows: u64, last_insert_id: Option<u64> },
    Info(Option<String>),
    ResultSetEnd,                          // 结果集边界
    Truncated,                             // 超 max_rows 截断（协议预留，流式截断发射归 #35/#27）
    Done,                                  // 命令成功收尾（unit 变体：序列化为 {"event":"done"}，无 data）
    Error { kind: String, message: String }, // 命令失败收尾（携带 kind，对齐 S5）
}
```

### 4.2 `CollectingSink` 分桶（`query.rs`）

```rust
pub struct CollectingSink {
    result_sets: Vec<ResultSet>,
    current: Option<ResultSetBuilder>,
    max_rows: usize,
    affected_rows: u64,          // 顶层值 = 最后一个结果集的值（MySQL 多语句语义）
    last_insert_id: Option<u64>,
    info: Option<String>,
}
// Columns → 结算上一个 current、开新 builder；Rows → push current（按 max_rows 截断）；
// Truncated → 标记 current；ResultSetEnd → 结算 current 入 result_sets；
// Affected → 记顶层 affected_rows/last_insert_id；Info → info；Done/Error → 终态。
// into_output：先结算残留 current 再产出 QueryOutput。
```

### 4.3 驱动遍历全部结果集（`crates/dby-driver-mysql/src/lib.rs`）

mysql_async 0.37 的 `QueryResult` **无 `next_result_set()`**，正确遍历如下（`next()` 在结果集边界返回 None 时已自动 `next_set()`；DML 集无列无行、`next()` 不推进，靠 `is_empty()` 判定）：

```rust
let mut qr = self.conn.query_iter(sql).await.map_err(db_err)?;
loop {
    // mysql_async 0.37：DML（OK 包 0x00）→ set_pending_result(Some(空列))（helpers.rs:69-74），
    // columns() 对 DML 返回 Some(空列) 而非 None，且空列集 next() 也会推进；故按「空/非空」判别。
    match qr.columns() {
        Some(cols) if !cols.is_empty() => {
            // SELECT 集
            let columns: Vec<ColumnInfo> = cols.iter().map(from_mysql_column).collect();
            sink.on_event(StreamEvent::Columns(columns));
            let mut batch = Vec::with_capacity(BATCH_ROWS);
            while let Some(row) = qr.next().await.map_err(db_err)? { // 集尾返回 None 自动 next_set
                batch.push(row_to_values(&row));
                if batch.len() >= BATCH_ROWS {
                    sink.on_event(StreamEvent::Rows(std::mem::take(&mut batch)));
                }
                if let Some(tok) = &opts.cancel {
                    if tok.is_cancelled() { return Err(DbError::Cancelled); }
                }
            }
            if !batch.is_empty() { sink.on_event(StreamEvent::Rows(batch)); }
            sink.on_event(StreamEvent::ResultSetEnd);
        }
        Some(_) => {
            // DML 集（空列）：先读 affected/last_insert_id 发 Affected，再 next() 推进（连续 DML 不丢集）
            sink.on_event(StreamEvent::Affected {
                affected_rows: qr.affected_rows(),
                last_insert_id: qr.last_insert_id(),
            });
            qr.next().await.map_err(db_err)?; // 空列集 next() 推进到下一集
        }
        None => {
            // 无 pending 结果；可能隐藏多语句中途 server 错误（helpers.rs:58-62 存为 pending error），
            // 再调一次 next() 用 ? 冒错（无 pending 则 Ok(None)），随后 break
            qr.next().await.map_err(db_err)?;
            break;
        }
    }
}
```

> `affected_rows/last_insert_id` 顶层值为「最后一个结果集」的值（MySQL 多语句下 `last_insert_id` 仅最后语句有意义），在 §7 注明。

### 4.4 `ChannelSink` 终止事件 + send 失败取消（#42）

```rust
struct ChannelSink {
    channel: Channel<StreamEvent>,
    rows: usize,
    cancel: Option<CancellationToken>, // 查询实例 token（#5/S1）
}

impl ResultSink for ChannelSink {
    fn on_event(&mut self, ev: StreamEvent) {
        if let StreamEvent::Rows(ref rows) = ev { self.rows += rows.len(); }
        if self.channel.send(ev).is_err() {
            if let Some(c) = &self.cancel { c.cancel(); } // 前端关 channel → 取消后端
        }
    }
}
```

`execute_query_stream` 末尾（**clone channel** 后再在 sink 消费后发终止，避免 sink 内 `send` 消费后引用失效）：

```rust
let term_channel = channel.clone(); // Tauri Channel 可 Clone
// ...execute_stream 完成后：
match result {
    Ok(()) => { let _ = term_channel.send(StreamEvent::Done); /* ... */ }
    Err(e) => { let _ = term_channel.send(StreamEvent::Error { kind: e.kind(), message: e.to_string() }); /* ... */ }
}
```

> send 失败触发取消覆盖 SELECT 与 DML（DML 取消依赖 #5/#34 的 `select!` 秒断）；用查询实例 token 而非连接级 token，避免 #23 毒化。

### 4.5 前端多结果集（`src/api.ts` / `src/store.ts` / `src/App.tsx` / `ResultsGrid.tsx`）

```ts
export interface StreamResult {
  columns: ColumnInfo[] | null;
  rows: CellValue[][];
  result_sets: ResultSet[];        // 新增：多结果集
  current_set: number;             // 新增：当前展示集游标
  affected_rows: number;
  last_insert_id: number | null;
  truncated: boolean;
}
```

- `channel.onmessage` 增加分支：`result_set_end`（结算当前集入 `result_sets`）、`truncated`、`done`（复位 `running` + 状态栏）、`error`（置 `ws.error`）。
- **`running` 复位**：改为由 `done`/`error` 事件复位；`finally { running:false }` 移除（原 `App.tsx:182`），避免「invoke 返回」与「channel 终态」双路复位竞态。`invoke` 返回后的 `setStatus`（`App.tsx:176-177`）改为在 `done` 处理器内设置。
- `api.ts` 的 `StreamEvent` 联合类型同步（`done` 为无 data 的 unit；`error` 为 `{kind,message}`）。

## 5. 错误处理（遵循 S5）

- `StreamEvent::Error{kind,message}` 携带 kind（`database/…/cancelled`），前端据 kind 区分「取消」与「失败」（联动 #29）。
- `channel.send` 失败：触发 `cancel`，后端查询尽早 `Cancelled` 结束。

## 6. 测试策略

- **单元（query）**：`CollectingSink` 两 `Columns` 之间分桶、`ResultSetEnd` 结算、`max_rows` 截断 + `Truncated`、`into_output` 残留结算、`Affected` 记顶层值。
- **单元（channel）**：`ChannelSink.send` 失败触发 cancel（用可注入失败路径的抽象，Tauri Channel 难直接单测）。
- **集成（`#[ignore]`）**：`CALL` 存储过程 2 结果集 → `result_sets.len()==2`；多语句 `SELECT 1; SELECT 2`；**多 DML 语句**（顶层 affected/last_insert_id 语义）；空结果集；**前端 `done` 复位时序**。

## 7. 回归风险与影响面

- `StreamEvent` 加变体：前端 `switch` 覆盖新 case（TS exhaustiveness 提示）。
- `QueryOutput.result_sets` 恒 1 → 多组：导出/编辑等只取第一组的路径确认无破坏。
- `affected_rows/last_insert_id` 顶层语义 = 最后一结果集（多语句 MySQL 语义），前端展示按此解释。
- `running` 复位来源改动：`finally` 移除后，若 channel 未发 `done`（异常路径）需在 `error`/invoke 拒绝路径兜底复位。

## 8. 关联缺陷处置

- #28：4.1/4.2/4.3；#42：4.4/4.5。

## 9. 与其它方案组的依赖

- 提供 S4 落地；依赖 #5（S1 查询实例 token + #34 DML 取消 + #23 免毒化）；依赖 S5（错误 kind）。
