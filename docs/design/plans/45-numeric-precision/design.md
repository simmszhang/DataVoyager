# #45 数值精度 — 设计文档

> 状态：评审需修订（2 阻断项已修订，待复审） · 优先级 P1 · 规模：大 · 关联缺陷：#1 联动 · 依赖共享契约：S2（类型映射层）、S5（错误形状）

## 1. 现状与影响

- `value.rs:13-14`：`Value::I64(i64)`/`U64(u64)` 经 `#[serde(tag="t",content="v")]` 序列化为 JSON **number**。
- `query.rs:77`：`QueryOutput.last_insert_id: Option<u64>`（**数据库值**，BIGINT AUTO_INCREMENT 可超 2^53）。
- `commands.rs:22`：`ConnectResponse.id: u64`（**内部计数器**，`state.alloc_id()` 自增，非数据库值，不会超 2^53）。
- 前端 `api.ts`：`CellValue` 的 `i64/u64` 为 `number`、`last_insert_id` 为 `number`；`toCellValue`（`App.tsx:14-20`）用正则 + `Number()` 解析编辑输入（丢精度的真正前端点）。
- `value.rs:96-97`：`to_json_value`（JSON 导出）对 `I64/U64` 输出 JSON number。
- **影响**：BIGINT/BIGINT UNSIGNED 主键超 2^53 静默丢精度（展示/编辑/导出）；`last_insert_id` 超 2^53 丢精度（review S11）。

## 2. 目标与成功标准

1. `Value::I64/U64` 跨 IPC 以**字符串**承载（envelope `{"t":"i64","v":"<十进制字符串>"}`）。
2. `last_insert_id`（数据库值）以字符串返回；`ConnectResponse.id`/`ConnectionSummary.id`（内部计数器）**保持 number**（非数据库值，无 2^53 风险，字符串化徒增连锁破坏）。
3. 前端 `i64/u64` 字符串渲染；编辑输入 `toCellValue` 改为按列类型解析（联动 #1），不再 `Number()`。
4. JSON 导出对 `I64/U64` 输出字符串，保持无损。
5. 成功标准：`BIGINT(9223372036854775807)` 查询→展示→编辑→回写无损；`last_insert_id` 超 2^53 不丢精度。

## 3. 方案对比

### 方案 A：envelope `v` 改字符串（完整手写 serde）
- `Value::I64/U64` 的 `v` 输出字符串，反序列化按字符串解析回 `i64/u64`；其余 14 变体保持原 tag/content 语义。
- **优点**：契约简单、前端当字符串、无歧义。**缺点**：破坏性契约变更，需整体手写 Serialize/Deserialize。

### 方案 B：保留 number + 增 `s` 字段
- **缺点**：双字段冗余、`v` 仍丢精度误导消费方，否决。

### 方案 C：前端 BigInt 解析 number
- **缺点**：JSON number 在后端序列化/前端 `JSON.parse` 环节已丢精度，无法根治，否决。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 `Value` 整体手写 serde（`crates/dby-core/src/value.rs`）

去掉 `#[serde(tag="t", content="v", ...)]`，改为完整手写 `Serialize`/`Deserialize`（**覆盖全部 16 变体**，仅 `I64/U64` 的 `v` 为字符串，其余不变）：

```rust
impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(2))?;
        match self {
            Value::Null => { m.serialize_entry("t", "null")?; }
            Value::Bool(b) => { m.serialize_entry("t", "bool")?; m.serialize_entry("v", b)?; }
            Value::I64(i) => { m.serialize_entry("t", "i64")?; m.serialize_entry("v", &i.to_string())?; }
            Value::U64(u) => { m.serialize_entry("t", "u64")?; m.serialize_entry("v", &u.to_string())?; }
            Value::F64(f) => { m.serialize_entry("t", "f64")?; m.serialize_entry("v", f)?; }
            Value::Decimal(s) | Value::Str(s) | Value::Date(s) | Value::Time(s) | Value::DateTime(s) | Value::Uuid(s) => {
                m.serialize_entry("t", self.kind())?; m.serialize_entry("v", s)?;
            }
            Value::Bytes(b) => { m.serialize_entry("t", "bytes")?; m.serialize_entry("v", b)?; }
            Value::Json(j) => { m.serialize_entry("t", "json")?; m.serialize_entry("v", j)?; }
            Value::Array(a) => { m.serialize_entry("t", "array")?; m.serialize_entry("v", a)?; }
            Value::Map(mp) => { m.serialize_entry("t", "map")?; m.serialize_entry("v", mp)?; }
        }
        m.end()
    }
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V; // 手动映射访问器：读 "t" 与可选 "v"，按 tag 分派
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Value;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, "tagged value envelope") }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
                let mut t: Option<String> = None;
                let mut v: Option<serde_json::Value> = None;
                while let Some(k) = map.next_key::<String>()? {
                    match k.as_str() {
                        "t" => t = Some(map.next_value()?),
                        "v" => v = Some(map.next_value()?),
                        _ => { let _: serde::de::IgnoredAny = map.next_value()?; }
                    }
                }
                let t = t.ok_or_else(|| serde::de::Error::missing_field("t"))?;
                let v = v.unwrap_or(serde_json::Value::Null);
                Ok(match t.as_str() {
                    "null" => Value::Null,
                    "bool" => Value::Bool(serde_json::from_value(v).map_err(serde::de::Error::custom)?),
                    "i64" => Value::I64(match v { serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom)?, serde_json::Value::Number(n) => n.as_i64().ok_or_else(|| serde::de::Error::custom("i64"))?, _ => return Err(serde::de::Error::custom("i64 v must be string/number")) }),
                    "u64" => Value::U64(match v { serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom)?, serde_json::Value::Number(n) => n.as_u64().ok_or_else(|| serde::de::Error::custom("u64"))?, _ => return Err(serde::de::Error::custom("u64 v must be string/number")) }),
                    "f64" => Value::F64(serde_json::from_value(v).map_err(serde::de::Error::custom)?),
                    "decimal" => Value::Decimal(as_string(v)?),
                    "str" => Value::Str(as_string(v)?),
                    "bytes" => Value::Bytes(serde_json::from_value(v).map_err(serde::de::Error::custom)?),
                    "date" => Value::Date(as_string(v)?),
                    "time" => Value::Time(as_string(v)?),
                    "datetime" => Value::DateTime(as_string(v)?),
                    "json" => Value::Json(v),
                    "uuid" => Value::Uuid(as_string(v)?),
                    "array" => Value::Array(serde_json::from_value(v).map_err(serde::de::Error::custom)?),
                    "map" => Value::Map(serde_json::from_value(v).map_err(serde::de::Error::custom)?),
                    _ => return Err(serde::de::Error::custom("unknown value tag")),
                })
            }
        }
        d.deserialize_map(V)
    }
}
```

> 关键点：手写 impl 覆盖全部 16 变体；`i64/u64` 的 `v` 反序列化**同时接受字符串与旧 number**（迁移过渡）；`kind()`/`as_string` 为辅助函数。既有 `value.rs` 单测（`serializes_as_tagged_envelope` 断言 `{"t":"i64","v":42}`）需改为断言字符串 `{"t":"i64","v":"42"}`。

`to_json_value`（导出 JSON）：`I64/U64` → `serde_json::Value::String(i.to_string())`（无损）。

### 4.2 `last_insert_id` 字符串化（仅此一处；`id` 不动）

- `query.rs:77`：`QueryOutput.last_insert_id: Option<String>`；`query.rs:94`：`StreamEvent::Affected.last_insert_id: Option<String>`；`query.rs:126`（`CollectingSink` 内部 `last_insert_id` 字段）同步改 `Option<String>`；驱动 `qr.last_insert_id().map(|v| v.to_string())`。
- **`ConnectResponse.id`/`ConnectionSummary.id` 保持 `u64`**：它们是 `alloc_id()` 内部计数器（`state.rs:47-49`），非数据库值、不会超 2^53，字符串化无收益且会连锁破坏前端 `workspaces: Record<number, WorkspaceState>`/`tabs: number[]` 的 key 类型。

### 4.3 前端（`src/api.ts` / `App.tsx` / `ResultsGrid.tsx` / `store.ts`）

```ts
export type CellValue =
  | { t: "null" } | { t: "bool"; v: boolean }
  | { t: "i64"; v: string } | { t: "u64"; v: string }   // 字符串
  | { t: "f64"; v: number } | { t: "decimal"; v: string }
  | /* ...其余不变 */;
// last_insert_id: string | null; ConnectResponse.id 保持 number
```

- `displayCell`（`api.ts:22-48`）：i64/u64 直接返回 `v`（字符串），不做 `Number()`。
- **编辑路径**：真正丢精度的前端点是 `toCellValue`（`App.tsx:14-20`，用 `Number()` 正则解析）；改为提交「输入串 + 列 `column_type`」，由后端 `parse_value`（#1）解析，前端不再 `Number()`。

### 4.4 与 #1 的关系

本方案的 envelope 字符串化**独立于** #1 的列类型判定（#1 决定「某列是 I64/U64」，本方案决定「I64/U64 如何跨 IPC 编码」）；两者可并行，无先后依赖。#1 的 `parse_value` 负责把编辑输入按列类型解析回 `i64/u64`。

## 5. 错误处理（遵循 S5）

- 反序列化 `v` 非数字字符串：`serde::de::Error::custom`（反序列化错误）。
- 编辑输入超 `i64/u64` 范围：`parse_value`（#1）返回 `DbError::Other`。

## 6. 测试策略

- **单元（value）**：`I64(9223372036854775807)` 序列化为 `{"t":"i64","v":"9223372036854775807"}` 并往返无损；`to_json_value` 大整数输出字符串；**更新既有 `serializes_as_tagged_envelope`/`roundtrips_through_json` 断言**（number→string）。
- **单元（query）**：`last_insert_id` 字符串往返。
- **前端（手工）**：`displayCell` 大整数不丢精度；`toCellValue` 不再 `Number()`。
- **集成（`#[ignore]`）**：`BIGINT`/`BIGINT UNSIGNED` 主键查询 + `last_insert_id` 超 2^53 无损。

## 7. 回归风险与影响面

- **破坏性 IPC 契约变更**：`CellValue.i64/u64.v` number→string，前端 `v` 消费点同步（`displayCell`、编辑、导出）。
- `ConnectResponse.id` **不变**（保持 number），前端 key 类型无连锁破坏。
- `last_insert_id` 类型变化影响状态栏展示。
- 导出语义不对称：`to_json_value` 仅被 JSON 导出消费，改后 JSON 导出 `I64/U64` 为字符串；CSV/Markdown/INSERT 导出走 `to_display_string`（本就字符串），不受影响。形成「JSON=字符串、CSV/INSERT=数字文本」的不对称，需在导出文档注明。

## 8. 关联缺陷处置

- #45：4.1/4.2/4.3；#1 联动：4.4（`parse_value` 解析回 i64/u64）。

## 9. 与其它方案组的依赖

- 独立于 #1（可并行）；被 #28（`StreamEvent::Affected.last_insert_id` 字符串化）引用；依赖 S5。
