# #1 Value 类型映射 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 以列类型驱动值转换（**文本协议 Bytes 字符串解析**），精确产出 `Decimal/Bool/Json/Bytes/Date/Time/DateTime`，统一两条 type_name 路径，编辑提交携带列类型。

**Architecture:** 新增 `ColumnType`/`ColumnTypeBase`；`ColumnInfo` 加 `column_type`；`Dialect::parse_column_type`（元数据路径）+ `from_mysql_column`（查询结果路径）；`conv` 按「`MyValue::Bytes` 字符串 + 列类型」解析；编辑路径 `parse_value`。

**Tech Stack:** Rust（dby-core / dby-driver-mysql / serde / mysql_async 0.37）、React 19/TS。

**Spec:** `docs/design/plans/01-value-type-mapping/design.md`（修订版，含已核对的文本协议语义）

## Global Constraints

- **文本协议前提**：`query_iter` 下所有非 NULL 值只产 `MyValue::Bytes`（`TextProtocol::read_result_set_row` → `RowDeserializer<ServerSide, Text>`）；值转换必须按列类型**解析字符串**，非匹配 `MyValue` 强类型变体（后者为二进制协议预留死代码）。
- `Value` envelope（`{"t","v"}`）不变（数值精度字符串化归 #45，本批不动 envelope）。
- `ColumnInfo` 保留 `type_name`，新增 `column_type`。
- 错误形状遵循 S5（编辑解析失败用 `DbError::Other`）。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core -p dby-driver-mysql`、`pnpm build`。

---

### Task 1: `ColumnType` / `ColumnTypeBase`

**Files:**
- Create/Modify: `crates/dby-core/src/metadata.rs`

**Interfaces:**
- Produces: `ColumnTypeBase`（enum，`Copy` + `Default`（`#[default] Unknown`））、`ColumnType`（struct，含 `temporal_precision`，derive `Default`）、`ColumnInfo.column_type: Option<ColumnType>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn column_type_serializes_base_and_metadata() {
    let ct = ColumnType { base: ColumnTypeBase::Decimal, numeric_precision: Some(10),
        numeric_scale: Some(2), unsigned: false, char_max_length: None,
        temporal_precision: None, charset: None, collation: None };
    let json = serde_json::to_value(&ct).unwrap();
    assert_eq!(json["base"], "decimal");
    assert_eq!(json["numeric_precision"], 10);
    assert_eq!(json["numeric_scale"], 2);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core metadata::tests::column_type_serializes_base_and_metadata`
Expected: FAIL（类型不存在）

- [ ] **Step 3: 最小实现**

按 design §4.1：`ColumnTypeBase` 加 `#[derive(Copy, Default)]` + `#[default] Unknown` 变体（避免 `match ct.base` move 借用内容）；`ColumnType` 加 `temporal_precision: Option<u32>` 并 `#[derive(Default)]`（`base` 默认 Unknown），使 `..Default::default()` 可在测试/兜底使用；`ColumnInfo` 增 `column_type`（`skip_serializing_if = "Option::is_none"`）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core metadata::tests::column_type_serializes_base_and_metadata`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/metadata.rs
git commit -m "feat(core): structured ColumnType (Copy+Default) + extend ColumnInfo (#32)"
```

---

### Task 2: `Dialect::parse_column_type` + `display_type_name`

**Files:**
- Modify: `crates/dby-core/src/dialect.rs`（trait 增方法）
- Modify: `crates/dby-driver-mysql/src/dialect.rs`（MySQL 实现）

**Interfaces:**
- Produces: `parse_column_type(&self, raw) -> Option<ColumnType>`、`display_type_name(&self, &ColumnType) -> String`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn parse_mysql_column_types() {
    use crate::ColumnTypeBase as B;
    let d = MysqlDialect;
    assert_eq!(d.parse_column_type("int(11) unsigned").map(|c| c.base), Some(B::I32));
    assert_eq!(d.parse_column_type("int(11) unsigned").map(|c| c.unsigned), Some(true));
    let dec = d.parse_column_type("decimal(10,2)").unwrap();
    assert_eq!(dec.base, B::Decimal);
    assert_eq!(dec.numeric_precision, Some(10));
    assert_eq!(dec.numeric_scale, Some(2));
    assert_eq!(d.parse_column_type("tinyint(1)").map(|c| c.base), Some(B::Bool));
    assert_eq!(d.parse_column_type("bigint unsigned").map(|c| c.base), Some(B::U64));
    assert_eq!(d.parse_column_type("varchar(255)").map(|c| c.char_max_length), Some(255));
    assert_eq!(d.parse_column_type("datetime(6)").map(|c| c.temporal_precision), Some(6));
    assert_eq!(d.parse_column_type("json").map(|c| c.base), Some(B::Json));
    assert_eq!(d.parse_column_type("blob").map(|c| c.base), Some(B::Bytes));
    assert_eq!(d.parse_column_type("bit").map(|c| c.base), Some(B::Bytes)); // bit → Bytes（与查询路径一致）
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql dialect::tests::parse_mysql_column_types`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design §4.3 表（大小写不敏感、提取 `(m,d)`/`unsigned`/`(fsp)` 长度；`bit`→`Bytes`）；`display_type_name(&ColumnType)` 如 `I64 unsigned → "bigint unsigned"`、`Decimal(10,2) → "decimal(10,2)"`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql dialect::tests::parse_mysql_column_types`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/dialect.rs crates/dby-driver-mysql/src/dialect.rs
git commit -m "feat(core): dialect-level column type parse + display (#20/#32)"
```

---

### Task 3: 文本协议值转换（#1/#60）

**Files:**
- Modify: `crates/dby-driver-mysql/src/conv.rs`

**Interfaces:**
- Consumes: `ColumnType`（Task 1）
- Produces: `mysql_value_to_dby(v: &MyValue, ct: &ColumnType) -> Value`（**`MyValue::Bytes` 字符串按列类型解析**）；保留 `mysql_value_to_dby_legacy` 作为 Unknown 回退

- [ ] **Step 1: 写失败测试（全部走 `MyValue::Bytes`，文本协议真实形态）**

```rust
fn ct(base: ColumnTypeBase) -> ColumnType { ColumnType { base, ..Default::default() } }

#[test]
fn int_column_parses_bytes_to_i64() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"42".to_vec()), &ct(ColumnTypeBase::I32)), Value::I64(42));
}
#[test]
fn unsigned_int_column_parses_to_u64() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"18446744073709551615".to_vec()), &ct(ColumnTypeBase::U64)), Value::U64(18446744073709551615));
}
#[test]
fn decimal_is_decimal_not_str() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"123.450".to_vec()), &ct(ColumnTypeBase::Decimal)), Value::Decimal("123.450".into()));
}
#[test]
fn blob_is_bytes_not_str() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(vec![0xff, 0xfe]), &ct(ColumnTypeBase::Bytes)), Value::Bytes(vec![0xff, 0xfe]));
}
#[test]
fn tinyint1_zero_one_bool_else_i64() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"1".to_vec()), &ct(ColumnTypeBase::Bool)), Value::Bool(true));
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"2".to_vec()), &ct(ColumnTypeBase::Bool)), Value::I64(2)); // 非 0/1 不坍缩
}
#[test]
fn midnight_datetime_stays_datetime() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"2024-01-02 00:00:00".to_vec()), &ct(ColumnTypeBase::DateTime)), Value::DateTime("2024-01-02 00:00:00".into()));
}
#[test]
fn time_string_kept_without_fraction() {
    assert_eq!(mysql_value_to_dby(&MyValue::Bytes(b"03:04:05".to_vec()), &ct(ColumnTypeBase::Time)), Value::Time("03:04:05".into()));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql conv::tests::int_column_parses_bytes_to_i64`
Expected: FAIL（当前 Bytes→Str 启发式，或 `_ => Null`）

- [ ] **Step 3: 最小实现**

按 design §4.4：`MyValue::NULL→Null`；`MyValue::Bytes(b)` → `s = from_utf8_lossy(b)`，按 `ct.base` 解析（整数族 parse i64/u64、F32/F64 parse f64、Decimal 保留原串、Bytes 原样、Json parse、Date/DateTime 字符串直通、Time 用 `normalize_time_str` 规范化（#60：剥 `us==0` 的 `.000000`）、Bool 仅 `"0"/"1"`、Unknown 回退 legacy）；`MyValue::Int/UInt/Date/Time` 等强类型分支保留为**二进制协议预留**（注释标注当前文本协议不产）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql conv::tests::`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/conv.rs
git commit -m "feat(mysql): text-protocol column-type-driven value parsing (#1/#60)"
```

---

### Task 4: 双构造器 + 两条路径统一（#33）

**Files:**
- Modify: `crates/dby-driver-mysql/src/lib.rs`（`columns()`、`execute_stream()`、`row_to_values`）
- Modify: `crates/dby-driver-mysql/src/conv.rs`（或 `lib.rs`）新增 `from_mysql_column`

**Interfaces:**
- Produces: `from_mysql_column(&mysql_async::Column) -> ColumnType`（枚举+flags+length+decimals+charset）
- Consumes: `parse_column_type`（Task 2）、`mysql_value_to_dby`（Task 3）

- [ ] **Step 1: 写失败测试（集成，`#[ignore]`）**

```rust
#[tokio::test]
#[ignore]
async fn metadata_and_query_result_type_names_agree() {
    // 建表 int unsigned / decimal(10,2) / datetime / tinyint(1)，分别走 columns() 与 execute_stream()，
    // 断言两者 ColumnInfo.column_type.base 与 unsigned 一致（不再 int vs long 不一致）
}
```

- [ ] **Step 2: 运行确认（起 MySQL 后）**

Run: `cargo test -p dby-driver-mysql --test mysql_integration -- --ignored metadata_and_query_result_type_names_agree`
Expected: FAIL（当前两路径 type_name 不同，且无 column_type）

- [ ] **Step 3: 最小实现**

- `columns()`（元数据路径）：`COLUMN_TYPE` 字符串 → `MysqlDialect.parse_column_type`；扩展 SELECT 补 `NUMERIC_PRECISION, NUMERIC_SCALE, DATETIME_PRECISION, CHARACTER_MAXIMUM_LENGTH, CHARACTER_SET_NAME, COLLATION_NAME`。
- `execute_stream()`（查询结果路径）：`cols.iter().map(from_mysql_column)`——用 `c.column_type()` 枚举（TINY + `column_length()==1` → `Bool`）、`flags().contains(UNSIGNED_FLAG)`、`column_length()/decimals()`、`character_set()==63` 判二进制；`let mut base` 供 `to_unsigned` 改写。
- `row_to_values(&row, &column_types)` 按列转换（**每次 `row.get::<Value, usize>(i)` 得 `Bytes`，经 `mysql_value_to_dby(v, &column_types[i])`**）。
- `type_name` 两路径均由 `display_type_name(&column_type)` 生成。

- [ ] **Step 4: 运行确认通过**

Run: 同上集成测试
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/lib.rs crates/dby-driver-mysql/src/conv.rs
git commit -m "fix(mysql): dual constructors unify metadata & query column types (#33)"
```

---

### Task 5: 编辑路径携带列类型（#11/#69，含主键列）

**Files:**
- Modify: `crates/dby-core/src/edit.rs`（或 `metadata.rs`）新增 `parse_value`
- Modify: `src-tauri/src/commands.rs`（`build_edit_sql`/`execute_edit` 或新增 `parse_cell_value`）
- Modify: `src/App.tsx`（`handleEditCell` 不再正则 `toCellValue`）、`src/api.ts`

**Interfaces:**
- Produces: `pub fn parse_value(input: &str, ct: &ColumnType) -> Result<Value>`（解析失败 `DbError::Other`）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn parse_value_by_column_type() {
    let i64ct = ColumnType { base: ColumnTypeBase::I64, ..Default::default() };
    assert_eq!(parse_value("42", &i64ct).unwrap(), Value::I64(42));
    assert!(parse_value("abc", &i64ct).is_err());
    let decct = ColumnType { base: ColumnTypeBase::Decimal, ..Default::default() };
    assert_eq!(parse_value("1.50", &decct).unwrap(), Value::Decimal("1.50".into()));
    let date_ct = ColumnType { base: ColumnTypeBase::Date, ..Default::default() };
    assert_eq!(parse_value("2024-01-02", &date_ct).unwrap(), Value::Date("2024-01-02".into()));
    assert!(parse_value("garbage", &date_ct).is_err()); // 时间类校验格式，不产出未校验 Str
    let jsonct = ColumnType { base: ColumnTypeBase::Json, ..Default::default() };
    assert_eq!(parse_value("{\"a\":1}", &jsonct).unwrap(), Value::Json(serde_json::json!({"a":1})));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core edit::tests::parse_value_by_column_type`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design §4.6 实现 `parse_value`（`match &ct.base` 避免 move；`Bytes => t.as_bytes().to_vec()`；错误 `DbError::Other`）；**pk 与 set 两处**都携带 `(列名, ColumnType, 输入串)` 经 `parse_value` 解析；`App.tsx` 的 `toCellValue` 改为提交「输入串 + column_type」。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core` + `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/edit.rs src-tauri/src/commands.rs src/App.tsx src/api.ts
git commit -m "feat(edit): parse cell input by column type incl. pk columns (#11/#69)"
```

---

### Task 6: 前端新变体渲染 + 集成回归

**Files:**
- Modify: `src/api.ts`（`ColumnInfo` 增 `column_type`；`displayCell` 处理 `decimal/json/bytes/bool`）、`src/components/ResultsGrid.tsx`

**Interfaces:**
- Consumes: `Value::Decimal/Json/Bytes/Bool` 的 envelope

- [ ] **Step 1: 写失败测试（手工）**

`displayCell` 对 `{t:"decimal",v:"1.50"}` 原样显示 `1.50`（不转 f64）；`{t:"bool",v:true}` 显示 `true`；`{t:"bytes",v:[222,173]}` 显示 `0xdead`。

- [ ] **Step 2: 运行确认失败**

Run: `pnpm build`
Expected: 确认 `ResultsGrid` 编辑控件按 `column_type.base` 选择

- [ ] **Step 3: 最小实现**

`ResultsGrid` 按 `column_type.base` 选编辑控件（numeric → number input、bool → checkbox、json → textarea、其余 text）；`displayCell` 保持字符串精确显示。

- [ ] **Step 4: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/api.ts src/components/ResultsGrid.tsx
git commit -m "feat(frontend): render Decimal/Bool/Json/Bytes with type-aware editors (#1)"
```
