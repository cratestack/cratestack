//! `JsonFilter` rendering via SQLite's `json_extract` (`json1` extension,
//! bundled by rusqlite). The schema-static `key` is inlined as part of the
//! `'$.<key>'` JSON path, with a defense-in-depth guard against any key
//! containing a single quote.

use std::fmt::Write;

use cratestack_sql::{Dialect, FilterOp, FilterValue, SqlValue};

pub(super) fn render_json(
    dialect: &dyn Dialect,
    filter: &cratestack_sql::JsonFilter,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    match filter {
        cratestack_sql::JsonFilter::HasKey { column, key } => {
            let json_path = json_path_literal(key);
            let _ = write!(sql, "json_extract({column}, '{json_path}') IS NOT NULL");
        }
        cratestack_sql::JsonFilter::GetText {
            column,
            key,
            op,
            value,
        } => {
            let json_path = json_path_literal(key);
            let _ = write!(sql, "json_extract({column}, '{json_path}')");
            match op {
                FilterOp::Eq => {
                    render_json_text_binary(dialect, "=", value, sql, binds, bind_index)
                }
                FilterOp::Ne => {
                    render_json_text_binary(dialect, "!=", value, sql, binds, bind_index)
                }
                FilterOp::Lt => {
                    render_json_text_binary(dialect, "<", value, sql, binds, bind_index)
                }
                FilterOp::Lte => {
                    render_json_text_binary(dialect, "<=", value, sql, binds, bind_index)
                }
                FilterOp::Gt => {
                    render_json_text_binary(dialect, ">", value, sql, binds, bind_index)
                }
                FilterOp::Gte => {
                    render_json_text_binary(dialect, ">=", value, sql, binds, bind_index)
                }
                FilterOp::IsNull => sql.push_str(" IS NULL"),
                FilterOp::IsNotNull => sql.push_str(" IS NOT NULL"),
                FilterOp::In | FilterOp::Contains | FilterOp::StartsWith | FilterOp::EqOrNull => {
                    unreachable!("JsonFilter::GetText built with unsupported op {:?}", op,);
                }
            }
        }
    }
}

fn render_json_text_binary(
    dialect: &dyn Dialect,
    operator: &str,
    value: &FilterValue,
    sql: &mut String,
    binds: &mut Vec<SqlValue>,
    bind_index: &mut usize,
) {
    let FilterValue::Single(value) = value else {
        unreachable!("json_get_text comparison requires FilterValue::Single");
    };
    let _ = write!(sql, " {operator} ");
    dialect.write_placeholder(sql, *bind_index);
    *bind_index += 1;
    binds.push(value.clone());
}

fn json_path_literal(key: &str) -> String {
    if key.contains('\'') {
        panic!(
            "JSON path key {key:?} contains a single quote; refusing to render to SQLite json_extract",
        );
    }
    format!("$.{key}")
}
