# #1 Value 类型映射 — 设计文档

> 状态：评审需修订（2 阻断项已修订，待复审） · 优先级 P1 · 规模：大 · 关联缺陷：#20/#32/#33/#60/#11/#69、#45 联动 · 依赖共享契约：S2（类型映射层）、S5（错误形状）

## 1. 现状与影响

- `conv.rs:10-37`：`mysql_value_to_dby` 是启发式——`Bytes` 可 UTF-8 解码即 `Str`（DECIMAL 落 `Str`、BLOB/BINARY 偶发误判 `Str`）；`Date(y,mo,d,h,mi,s,us)` 靠 `h==0&&mi==0&&s==0&&us==0` 判 `Date`，**午夜 DATETIME 被误判 Date**；`Decimal/Bool/Json/Uuid/Array/Map` 六个变体从不产出（JSON 列恒 `Str`）。review D1。
- `metadata.rs:16-27`：`ColumnInfo` 只有 `type_name: String`，无精度/scale/unsigned/charset/序位（#32，review C5）。
- `lib.rs:145`（元数据路径用 `COLUMN_TYPE` 如 `"int(11) unsigned"`）与 `lib.rs:269`（查询结果路径用 `format!("{:?}", c.column_type())` 如 `"long"`）两条 `type_name` 不一致（#33，review D3）。注：`c.column_type()` 是 mysql_async 的**枚举**，`format!("{:?}")` 发生在调用点 `lib.rs:269`；`MysqlDialect::display_type_name`（`dialect.rs:26-30`）本体只做 `strip_prefix("MYSQL_TYPE_")` + `to_lowercase`。
- `conv.rs:32-35`：`Time` 恒带 `.000000`、`"{sign}{days} ..."` 与契约不符（#60，review D2）。
- `App.tsx:14-20`：`toCellValue` 正则判 NULL/整数/浮点/字符串，不感知列类型（#11/#69，review F16）。
- **影响**：结果网格类型着色/编辑控件不准确；跨库不通用；DECIMAL 精度、午夜 DATETIME、BLOB、TIME 均失真。

## 2. 目标与成功标准

1. 值转换由**列类型驱动**，精确产出 `Decimal/Json/Bytes/Date/Time/DateTime`，且**不因转换丢数据**（`TINYINT(1)` 非 0/1 值不得坍缩为 Bool）。
2. 两条路径（元数据 / 查询结果）产出的 `ColumnType` **结构等价**（字段级可对齐），消除 `int` vs `long` 不一致。
3. `Dialect` 提供「原生类型名 → `ColumnType`」解析与「`ColumnType` → 展示名」，替代 Debug 字符串（#20）。
4. 编辑提交携带列类型（含主键列），后端把输入解析为正确 `Value`（#11/#69）。
5. `Time` 格式符合契约（#60，含 >24h 与负值）。
6. 成功标准：`DECIMAL(10,2)`→`Decimal`；`BLOB`→`Bytes`；`JSON`→`Json`；`TINYINT(1)` 值 0/1→`Bool`、值 2→`I64`（不坍缩）；午夜 `DATETIME`→`DateTime`；`int`/`int unsigned` 两路径 `ColumnType.base` 一致。

## 3. 方案对比

### 方案 A：结构化 `ColumnType` + 双构造器（推荐）
- 新增 `ColumnType`/`ColumnTypeBase`；**元数据路径**用 `parse_column_type(&str)`（解析 `COLUMN_TYPE` 字符串），**查询结果路径**用 `from_mysql_column(&mysql_async::Column)`（枚举 + flags + length + decimals）；转换按 `(value, column_type)` 产出 `Value`。
- **优点**：真正「类型映射层」，两路径都能产出结构化类型。**缺点**：改动面大。

### 方案 B：仅修转换启发式（不引入结构化列类型）
- 在 `mysql_value_to_dby` 里按 `type_name` 字符串 `match` 分支。
- **缺点**：仍依赖字符串 `type_name`，两路径不一致未解决，跨库不通用，返工。

### 方案 C：二进制协议 + mysql_async 类型元数据
- 切二进制协议拿原生类型。
- **缺点**：mysql_async 0.37 协议/类型面改动大、风险高，仍要落到统一 `Value`，不能替代 S2。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 结构化列类型（`crates/dby-core/src/metadata.rs` 或新 `types.rs`）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnTypeBase {
    Bool, I8, I16, I32, I64, U8, U16, U32, U64, F32, F64,
    Decimal, Str, Bytes, Date, Time, DateTime, Json, Uuid, Array, Map, Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnType {
    pub base: ColumnTypeBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_precision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_scale: Option<u32>,
    #[serde(default)]
    pub unsigned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_max_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_precision: Option<u32>, // datetime(6)/timestamp(6) 的小数秒位
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}
```

> `ColumnTypeBase` 为 `Copy`（避免 `match ct.base` move 借用内容）。此定义与 S2 一致（S2 增 `temporal_precision`）。

`ColumnInfo` 增字段：

```rust
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,                 // 保留：展示名，由 display_type_name 生成
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_type: Option<ColumnType>,   // 新增：结构化列类型（#32）
    // ...nullable/primary_key/default/comment 不变
}
```

### 4.2 `Dialect` 扩展（`crates/dby-core/src/dialect.rs`）

```rust
pub trait Dialect: Send + Sync {
    fn quote_identifier(&self, ident: &str) -> String;
    fn quote_string(&self, s: &str) -> String;
    fn limit_clause(&self, limit: Option<u64>, offset: Option<u64>) -> String;
    fn parse_column_type(&self, raw: &str) -> Option<ColumnType>;   // 元数据路径
    fn display_type_name(&self, ct: &ColumnType) -> String;
}
```

### 4.3 双构造器

**元数据路径**（`MysqlDialect::parse_column_type`，解析 `COLUMN_TYPE` 字符串）：

| 原生（大小写不敏感） | `base` | 备注 |
| --- | --- | --- |
| `tinyint(1)` | `Bool`（仅值 0/1 才产出 Bool，见 4.4） | 其余 `tinyint` → `I8` |
| `smallint` → `I16`；`mediumint` → `I32`；`int`/`integer` → `I32`；`bigint` → `I64` | 对应整数 base | `unsigned` 后缀 → 对应 U 变体（`unsigned` 布尔位 + base 取 U 族） |
| `decimal`/`numeric(m,d)` | `Decimal` | 记 `numeric_precision/scale` |
| `float` → `F32`；`double` → `F64` | | |
| `char`/`varchar`/`text`/`enum`/`set` | `Str` | `varchar(n)` 记 `char_max_length`；`text` 类长度由 information_schema 补 |
| `binary`/`varbinary`/`blob`/`*blob` | `Bytes` | |
| `date` → `Date`；`time` → `Time`；`datetime`/`timestamp` → `DateTime` | | `(fsp)` 记 `temporal_precision` |
| `bit` → `Bytes` | | BIT(n) 位掩码（文本协议下为原始字节，非 `"0"/"1"` 字符串；与查询路径 `from_mysql_column` 的 `MYSQL_TYPE_BIT→Bytes` 一致） |
| `json` → `Json`；`year` → `I16` | | YEAR 4 位 1901–2155，I16 够用 |
| spatial（`point`/`geometry` 等）→ `Bytes` | | |
| 其它 | `Unknown` | 回退启发式 |

`columns()` SQL（`lib.rs:137-139`）扩展 SELECT 增加 `NUMERIC_PRECISION, NUMERIC_SCALE, DATETIME_PRECISION, CHARACTER_MAXIMUM_LENGTH, CHARACTER_SET_NAME, COLLATION_NAME`，用于填充 `parse_column_type` 无法从 `COLUMN_TYPE` 字符串取到的字段（charset/collation 名、`text` 长度、temporal 精度）。

**查询结果路径**（`conv.rs` 或 `lib.rs` 新增 `from_mysql_column(&mysql_async::Column) -> ColumnType`）：

```rust
pub fn from_mysql_column(c: &mysql_async::Column) -> ColumnType {
    use mysql_async::consts::{ColumnFlags, ColumnType as MCT};
    let is_tinyint1 = c.column_type() == MCT::MYSQL_TYPE_TINY && c.column_length() == 1;
    let mut base = match c.column_type() {
        MCT::MYSQL_TYPE_TINY => if is_tinyint1 { ColumnTypeBase::Bool } else { ColumnTypeBase::I8 },
        MCT::MYSQL_TYPE_SHORT => ColumnTypeBase::I16,
        MCT::MYSQL_TYPE_INT24 | MCT::MYSQL_TYPE_LONG => ColumnTypeBase::I32,
        MCT::MYSQL_TYPE_LONGLONG => ColumnTypeBase::I64,
        MCT::MYSQL_TYPE_NEWDECIMAL => ColumnTypeBase::Decimal,
        MCT::MYSQL_TYPE_VAR_STRING | MCT::MYSQL_TYPE_VARCHAR | MCT::MYSQL_TYPE_STRING
        | MCT::MYSQL_TYPE_ENUM | MCT::MYSQL_TYPE_SET => ColumnTypeBase::Str,
        MCT::MYSQL_TYPE_TINY_BLOB | MCT::MYSQL_TYPE_MEDIUM_BLOB | MCT::MYSQL_TYPE_LONG_BLOB
        | MCT::MYSQL_TYPE_BLOB => ColumnTypeBase::Bytes,
        MCT::MYSQL_TYPE_JSON => ColumnTypeBase::Json,
        MCT::MYSQL_TYPE_DATE | MCT::MYSQL_TYPE_NEWDATE => ColumnTypeBase::Date,
        MCT::MYSQL_TYPE_TIME => ColumnTypeBase::Time,
        MCT::MYSQL_TYPE_DATETIME | MCT::MYSQL_TYPE_TIMESTAMP => ColumnTypeBase::DateTime,
        MCT::MYSQL_TYPE_BIT => ColumnTypeBase::Bytes, // BIT(n) 位掩码
        MCT::MYSQL_TYPE_FLOAT => ColumnTypeBase::F32,
        MCT::MYSQL_TYPE_DOUBLE => ColumnTypeBase::F64,
        _ => ColumnTypeBase::Unknown,
    };
    // BINARY/VARBINARY：文本协议下 column_type 为 STRING/VAR_STRING 且 charset=63（二进制）
    if base == ColumnTypeBase::Str && c.character_set() == 63 {
        base = ColumnTypeBase::Bytes;
    }
    let unsigned = c.flags().contains(ColumnFlags::UNSIGNED_FLAG);
    if unsigned { base = to_unsigned(base); } // to_unsigned: I8→U8 … I64→U64（Bool/其它透传）
    ColumnType {
        base,
        unsigned,
        numeric_precision: Some(c.column_length() as u32),
        numeric_scale: Some(c.decimals() as u32),
        char_max_length: Some(c.column_length() as u32), // 字节长度（查询路径仅能取到字节长度）
        temporal_precision: if c.decimals() > 0 { Some(c.decimals() as u32) } else { None },
        charset: Some(c.character_set().to_string()),    // 仅数字 id（63=二进制）
        collation: None,                                 // 查询路径无 collation 名
    }
}
```

**两路径残余差异（显式声明，不承诺逐字段完全相同）**：charset（元数据路径有名字、查询路径仅数字 id 63=二进制）、`decimal` precision/scale（元数据路径来自 `COLUMN_TYPE` 字符串、查询路径由 `column_length/decimals` 推断）、`text/blob` 长度（元数据路径来自 information_schema、查询路径仅字节长度）。**「统一」指 `base` 与 `unsigned` 两字段跨路径一致**，足以支撑类型驱动转换；其余字段为尽力填充、允许为 None。

### 4.4 列类型驱动转换（`crates/dby-driver-mysql/src/conv.rs`）

> **关键前提**：当前 `query_iter` 走**文本协议**（`queryable/mod.rs:63-73` `TextProtocol::read_result_set_row` → `RowDeserializer<ServerSide, Text>`），**所有非 NULL 值只产 `MyValue::Bytes`**（文本协议行数据无类型信息，`Value::Int/Date/Time` 等变体不经此路径）。因此转换必须**按列类型解析 `Bytes` 字符串**，而非匹配 `MyValue` 的强类型变体（后者为二进制协议预留、当前为死代码）。`Value` 枚举只有 `I64/U64/F64` 数值变体，窄整数/浮点列（I8..I32/U8..U32/F32）与宽类型产同一变体。

```rust
pub fn mysql_value_to_dby(v: &MyValue, ct: &ColumnType) -> Value {
    match v {
        MyValue::NULL => Value::Null,
        MyValue::Bytes(b) => {
            let s = String::from_utf8_lossy(b);
            match &ct.base {
                ColumnTypeBase::Bool => match s.trim() {
                    "0" => Value::Bool(false),
                    "1" => Value::Bool(true),
                    _ => if ct.unsigned { s.parse().map(Value::U64).unwrap_or_else(|_| Value::Str(s.into_owned())) }
                         else { s.parse().map(Value::I64).unwrap_or_else(|_| Value::Str(s.into_owned())) }, // TINYINT(1) 非 0/1 回退整数，不坍缩
                },
                ColumnTypeBase::I8 | ColumnTypeBase::I16 | ColumnTypeBase::I32 | ColumnTypeBase::I64 =>
                    if ct.unsigned { Value::U64(s.trim().parse().unwrap_or(0)) } else { Value::I64(s.trim().parse().unwrap_or(0)) },
                ColumnTypeBase::U8 | ColumnTypeBase::U16 | ColumnTypeBase::U32 | ColumnTypeBase::U64 =>
                    Value::U64(s.trim().parse().unwrap_or(0)),
                ColumnTypeBase::F32 | ColumnTypeBase::F64 => Value::F64(s.trim().parse().unwrap_or(0.0)),
                ColumnTypeBase::Decimal => Value::Decimal(s.into_owned()),   // 保留原串
                ColumnTypeBase::Str => Value::Str(s.into_owned()),
                ColumnTypeBase::Bytes => Value::Bytes(b.clone()),
                ColumnTypeBase::Json => serde_json::from_str(&s).map(Value::Json)
                    .unwrap_or_else(|_| Value::Str(s.into_owned())),
                ColumnTypeBase::Date => Value::Date(s.into_owned()),         // "YYYY-MM-DD"（列类型已定，不再按时分秒推断）
                ColumnTypeBase::Time => Value::Time(normalize_time_str(&s)), // "HH:MM:SS[.ffffff]" 规范化（#60）
                ColumnTypeBase::DateTime => Value::DateTime(s.into_owned()), // "YYYY-MM-DD HH:MM:SS[.ffffff]"
                _ => mysql_value_to_dby_legacy(v), // Unknown/Uuid/Array/Map → 现有启发式（Bytes→Str/Bytes）
            }
        }
        // 二进制协议预留（当前文本协议不产这些变体；未来切 BinaryProtocol 才命中）：
        MyValue::Int(i) => Value::I64(*i),
        MyValue::UInt(u) => Value::U64(*u),
        MyValue::Float(f) => Value::F64(*f as f64),
        MyValue::Double(d) => Value::F64(*d),
        MyValue::Date(y, mo, d, h, mi, s, us) => { /* 二进制路径按列类型：Date/DateTime 分支，参考既有 conv.rs 逻辑 */ }
        MyValue::Time(neg, days, h, mi, s, us) => { /* 二进制路径：归一化总小时数（#60） */ }
    }
}
```

`normalize_time_str`（#60，文本路径）：服务器文本 `TIME` 形如 `HH:MM:SS[.ffffff]`（仅 `fsp>0` 才带小数）；规范化到 `value.rs:23` 契约——`us==0` 时剥离 `.000000`，保留符号（负 TIME 服务器前缀 `-`）；超 24h（`>838:59:59` 之外的合法范围）服务器已以 `HHH:MM:SS` 给出，原样保留。

> **为何「午夜 DATETIME 误判 Date」在本方案消失**：文本路径下 Date/DATETIME 均以字符串到达，`ColumnTypeBase` 已由列类型判定（`from_mysql_column`/`parse_column_type`），不再靠时分秒启发式，故午夜 DATETIME 正确判 DateTime。

### 4.5 两条路径接线（#33）

- `columns()`（元数据路径）：`COLUMN_TYPE` 字符串 → `MysqlDialect.parse_column_type` → `ColumnInfo.column_type`；`type_name` 由 `display_type_name(&column_type)` 生成。
- `execute_stream()`（查询结果路径）：`cols.iter().map(from_mysql_column)` → 同一 `ColumnInfo.column_type`；`type_name` 同源生成。
- `row_to_values(&row, &column_types)` 按列转换。
- `parse_column_type` 返回 `None` 或 `from_mysql_column` 得 `Unknown` 时：`column_type = ColumnType::unknown()`（新增 `ColumnType::unknown() -> Self` 构造器，`base: Unknown` 其余 None/false），转换走 legacy，`display_type_name` 回退 `type_name` 原文。

### 4.6 编辑路径（#11/#69，含主键列）

`dby-core` 新增 `parse_value(input: &str, ct: &ColumnType) -> Result<Value>`：

```rust
pub fn parse_value(input: &str, ct: &ColumnType) -> Result<Value> {
    let t = input.trim();
    match &ct.base {
        ColumnTypeBase::Bool => match t {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            _ => Err(DbError::Other(format!("无法将 '{t}' 解析为 bool"))),
        },
        ColumnTypeBase::I8 | ColumnTypeBase::I16 | ColumnTypeBase::I32 | ColumnTypeBase::I64 =>
            t.parse::<i64>().map(Value::I64).map_err(|e| DbError::Other(e.to_string())),
        ColumnTypeBase::U8 | ColumnTypeBase::U16 | ColumnTypeBase::U32 | ColumnTypeBase::U64 =>
            t.parse::<u64>().map(Value::U64).map_err(|e| DbError::Other(e.to_string())),
        ColumnTypeBase::F32 | ColumnTypeBase::F64 =>
            t.parse::<f64>().map(Value::F64).map_err(|e| DbError::Other(e.to_string())),
        ColumnTypeBase::Decimal => Ok(Value::Decimal(t.to_string())), // 保留原串
        ColumnTypeBase::Date => validate_date(t).map(Value::Date),    // "YYYY-MM-DD"
        ColumnTypeBase::Time => validate_time(t).map(Value::Time),    // "HH:MM:SS[.ffffff]"
        ColumnTypeBase::DateTime => validate_datetime(t).map(Value::DateTime), // "YYYY-MM-DD HH:MM:SS[.ffffff]"
        ColumnTypeBase::Json => serde_json::from_str(t).map(Value::Json).map_err(|e| DbError::Other(e.to_string())),
        ColumnTypeBase::Bytes => Ok(Value::Bytes(t.as_bytes().to_vec())),
        _ => Ok(Value::Str(t.to_string())), // Str/Unknown 等
    }
}
```

- 时间类列：校验格式并产出 `Value::Date/Time/DateTime`（不再是未校验的 `Str`），失败返回 S5 形状错误。
- **主键列同样按类型解析**：`build_edit_sql`/`execute_edit` 的 `pk` 与 `set` 都携带 `(列名, ColumnType, 输入串)`，由 `parse_value` 统一解析（pk 的 BIGINT/UUID/DATE 主键不再走正则）。
- 前端 `handleEditCell` 提交「输入串 + 列 `column_type`」，删除 `toCellValue` 正则。

### 4.7 #45 联动（标注，不重复设计）

本方案定义「某列是 I64/U64」；`Value::I64/U64` 跨 IPC 字符串承载的 envelope 变更归 #45。本方案只保证转换层正确判定列类型。

## 5. 错误处理（遵循 S5）

- `parse_value` 解析失败：`DbError::Other("无法将 '<input>' 解析为 <类型>")`（输入错误语义上属 `Other`，非 `Config`）。
- JSON 解析失败回退 `Str`（读路径不阻断查询）。

## 6. 测试策略

- **单元（conv）**：DECIMAL→Decimal、BLOB→Bytes、JSON→Json、TINYINT(1) 值 0/1→Bool、值 2→I64（不坍缩）、午夜 DATETIME→DateTime、`Time` 超 24h/负值/`us==0` 归一化（#60）、`bigint unsigned`→U64、`int unsigned`→U64（经 unsigned 位）。
- **单元（dialect/from_mysql_column）**：`parse_column_type` 覆盖 `int(11) unsigned`/`decimal(10,2)`/`tinyint(1)`/`bigint unsigned`/`varchar(255)`/`bit(1)`；`from_mysql_column` 覆盖 LONG/LONGLONG/NEWDECIMAL/BLOB/JSON/DATE/TIME/DATETIME/UNSIGNED_FLAG。
- **单元（parse_value）**：整型/浮点/布尔/decimal/json/bytes/date/time/datetime 输入解析 + 失败路径。
- **集成（`#[ignore]`）**：`DECIMAL`/`JSON`/`TINYINT(1)` 非 0/1/`DATETIME` 午夜/`BLOB`/`BIGINT` 列查询，断言 `Value` 变体正确；元数据路径与查询结果路径 `column_type.base` 一致。

## 7. 回归风险与影响面

- `ColumnInfo` 增 `column_type`、`type_name` 改由 `display_type_name` 生成：**元数据路径原始展示串 `"int(11) unsigned"` 将丢失**（Schema 树类型列显示变化，宽度/unsigned 显示由新展示名决定），需在 `display_type_name` 里保留等价展示（如 `int unsigned`）。
- `Dialect` trait 增 `parse_column_type` 且 `display_type_name` 改签名：**所有 `Dialect` 实现者**（含测试 `TestDialect`、未来驱动）需同步实现，否则编译失败。
- `Value` 新变体（`Decimal/Json/Bytes/Bool`）开始产出：前端 `displayCell`（`api.ts:22-48`）已能展示这些变体，真正缺口是**编辑控件与着色**（F16/#69），§4.6 已覆盖编辑。
- `build_edit_sql`/`execute_edit` 入参从 `Value` 改为携带列类型：前端编辑流同步（pk/set 两处）。
- `parse_column_type` 返回 `None`/`Unknown` 的兜底路径（走 legacy + 原文 type_name）已定义。

## 8. 关联缺陷处置

- #20：4.2/4.3 `display_type_name(&ColumnType)`；#32：4.1 `ColumnInfo.column_type` + columns() 扩展 select；#33：4.5 双构造器统一；#60：4.4 `format_time`；#11/#69：4.6 编辑路径（含 pk）；#45：4.7 联动标注。

## 9. 与其它方案组的依赖

- 提供 S2 落地（含 `temporal_precision` 扩展），是 #45（数值精度）的前置；与 #4（方言感知）共享 `Dialect` trait 扩展，各自新增方法不冲突；依赖 S5。
