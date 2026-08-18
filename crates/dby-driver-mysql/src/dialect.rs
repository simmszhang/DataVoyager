//! MySQL 方言。

use dby_core::dialect::Dialect;
use dby_core::metadata::{ColumnType, ColumnTypeBase};

#[derive(Debug, Default, Clone, Copy)]
pub struct MysqlDialect;

impl Dialect for MysqlDialect {
    fn quote_identifier(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }

    fn quote_string(&self, s: &str) -> String {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    fn limit_clause(&self, limit: Option<u64>, offset: Option<u64>) -> String {
        match (limit, offset) {
            (Some(l), Some(o)) => format!("LIMIT {o}, {l}"),
            (Some(l), None) => format!("LIMIT {l}"),
            (None, Some(o)) => format!("LIMIT {o}, 18446744073709551615"),
            (None, None) => String::new(),
        }
    }

    /// 元数据路径：解析 information_schema `COLUMN_TYPE` 字符串（如 `int(11) unsigned`）。
    /// 大小写不敏感；提取 `(m,d)` / `unsigned` / `(fsp)` 参数。
    fn parse_column_type(&self, raw: &str) -> Option<ColumnType> {
        let raw = raw.trim().to_ascii_lowercase();
        if raw.is_empty() {
            return None;
        }
        let unsigned = raw.split_whitespace().any(|w| w == "unsigned");
        let open = raw.find('(');
        let name = match open {
            Some(i) => raw[..i].trim(),
            None => raw.trim(),
        }
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
        let params: Vec<&str> = match open {
            Some(i) => raw[i + 1..]
                .find(')')
                .map(|j| raw[i + 1..i + 1 + j].split(',').map(|s| s.trim()).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };

        let mut base = match name.as_str() {
            "tinyint" => {
                if params.first().map(|s| *s == "1").unwrap_or(false) {
                    ColumnTypeBase::Bool
                } else {
                    ColumnTypeBase::I8
                }
            }
            "smallint" => ColumnTypeBase::I16,
            "mediumint" => ColumnTypeBase::I32,
            "int" | "integer" => ColumnTypeBase::I32,
            "bigint" => ColumnTypeBase::I64,
            "float" => ColumnTypeBase::F32,
            "double" => ColumnTypeBase::F64,
            "decimal" | "numeric" => ColumnTypeBase::Decimal,
            "char" | "varchar" | "text" | "enum" | "set" => ColumnTypeBase::Str,
            "binary" | "varbinary" | "blob" | "tinyblob" | "mediumblob" | "longblob" => {
                ColumnTypeBase::Bytes
            }
            "date" => ColumnTypeBase::Date,
            "time" => ColumnTypeBase::Time,
            "datetime" | "timestamp" => ColumnTypeBase::DateTime,
            "bit" => ColumnTypeBase::Bytes,
            "json" => ColumnTypeBase::Json,
            "year" => ColumnTypeBase::I16,
            // spatial 类型 → 原始字节（与查询路径一致）
            "point" | "linestring" | "polygon" | "multipoint" | "multilinestring"
            | "multipolygon" | "geometry" | "geometrycollection" => ColumnTypeBase::Bytes,
            _ => ColumnTypeBase::Unknown,
        };
        // R6（#33）：unsigned 整数列 base 取 U 族（与查询路径 from_mysql_column 一致）
        if unsigned {
            base = crate::conv::to_unsigned(base);
        }

        let (numeric_precision, numeric_scale) = if base == ColumnTypeBase::Decimal {
            (
                params.first().and_then(|s| s.parse::<u32>().ok()),
                params.get(1).and_then(|s| s.parse::<u32>().ok()),
            )
        } else {
            (None, None)
        };
        let char_max_length =
            if base == ColumnTypeBase::Str && matches!(name.as_str(), "varchar" | "char") {
                // varchar(n)/char(n) 记 char_max_length；text/enum/set 长度由 information_schema 补
                params.first().and_then(|s| s.parse::<u32>().ok())
            } else {
                None
            };
        let temporal_precision = if matches!(base, ColumnTypeBase::Time | ColumnTypeBase::DateTime)
        {
            params.first().and_then(|s| s.parse::<u32>().ok())
        } else {
            None
        };

        Some(ColumnType {
            base,
            numeric_precision,
            numeric_scale,
            unsigned,
            char_max_length,
            temporal_precision,
            charset: None,
            collation: None,
        })
    }

    /// 结构化列类型 → 展示名（如 `bigint unsigned`、`decimal(10,2)`）。
    fn display_type_name(&self, ct: &ColumnType) -> String {
        let mut s = match ct.base {
            ColumnTypeBase::Bool => "tinyint(1)",
            ColumnTypeBase::I8 => "tinyint",
            ColumnTypeBase::I16 => "smallint",
            ColumnTypeBase::I32 => "int",
            ColumnTypeBase::I64 => "bigint",
            ColumnTypeBase::U8 => "tinyint unsigned",
            ColumnTypeBase::U16 => "smallint unsigned",
            ColumnTypeBase::U32 => "int unsigned",
            ColumnTypeBase::U64 => "bigint unsigned",
            ColumnTypeBase::F32 => "float",
            ColumnTypeBase::F64 => "double",
            ColumnTypeBase::Decimal => "decimal",
            ColumnTypeBase::Str => "varchar",
            ColumnTypeBase::Bytes => "blob",
            ColumnTypeBase::Date => "date",
            ColumnTypeBase::Time => "time",
            ColumnTypeBase::DateTime => "datetime",
            ColumnTypeBase::Json => "json",
            ColumnTypeBase::Uuid => "uuid",
            ColumnTypeBase::Array => "array",
            ColumnTypeBase::Map => "map",
            ColumnTypeBase::Unknown => "unknown",
        }
        .to_string();

        match ct.base {
            ColumnTypeBase::Decimal => {
                if let Some(p) = ct.numeric_precision {
                    let d = ct.numeric_scale.unwrap_or(0);
                    s = format!("decimal({p},{d})");
                }
            }
            ColumnTypeBase::Str => {
                if let Some(n) = ct.char_max_length {
                    s = format!("varchar({n})");
                }
            }
            ColumnTypeBase::Time => {
                if let Some(fsp) = ct.temporal_precision {
                    s = format!("time({fsp})");
                }
            }
            ColumnTypeBase::DateTime => {
                if let Some(fsp) = ct.temporal_precision {
                    s = format!("datetime({fsp})");
                }
            }
            _ => {
                if ct.unsigned
                    && matches!(
                        ct.base,
                        ColumnTypeBase::I8
                            | ColumnTypeBase::I16
                            | ColumnTypeBase::I32
                            | ColumnTypeBase::I64
                    )
                {
                    s.push_str(" unsigned");
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dby_core::metadata::ColumnTypeBase as B;

    #[test]
    fn parse_mysql_column_types() {
        let d = MysqlDialect;
        // R6：#33 后 unsigned 整数列 base 取 U 族（与查询路径 from_mysql_column 一致）
        assert_eq!(
            d.parse_column_type("int(11) unsigned").map(|c| c.base),
            Some(B::U32)
        );
        assert_eq!(
            d.parse_column_type("int(11) unsigned").map(|c| c.unsigned),
            Some(true)
        );
        assert_eq!(
            d.parse_column_type("smallint unsigned").map(|c| c.base),
            Some(B::U16)
        );
        let dec = d.parse_column_type("decimal(10,2)").unwrap();
        assert_eq!(dec.base, B::Decimal);
        assert_eq!(dec.numeric_precision, Some(10));
        assert_eq!(dec.numeric_scale, Some(2));
        assert_eq!(
            d.parse_column_type("tinyint(1)").map(|c| c.base),
            Some(B::Bool)
        );
        assert_eq!(
            d.parse_column_type("bigint unsigned").map(|c| c.base),
            Some(B::U64)
        );
        assert_eq!(
            d.parse_column_type("varchar(255)")
                .and_then(|c| c.char_max_length),
            Some(255)
        );
        assert_eq!(
            d.parse_column_type("datetime(6)")
                .and_then(|c| c.temporal_precision),
            Some(6)
        );
        assert_eq!(d.parse_column_type("json").map(|c| c.base), Some(B::Json));
        assert_eq!(d.parse_column_type("blob").map(|c| c.base), Some(B::Bytes));
        assert_eq!(d.parse_column_type("bit").map(|c| c.base), Some(B::Bytes)); // bit → Bytes（与查询路径一致）
    }
}
