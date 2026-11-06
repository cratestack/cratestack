//! A [`FilterExpr`] decided in memory against one record's column values,
//! with SQL's semantics: three-valued (a comparison with `NULL` is unknown,
//! and a `WHERE` keeps a row only when its condition is true), `LIKE`
//! patterns with `%` and `_`. Used where a filter applies to a record that
//! was never queried: a model event on a filtered subscription
//! (`docs/design/subscriptions-websocket.md` §5.3).
//!
//! Only the scalar subset is supported: comparisons, `IN`, `LIKE`-based
//! `contains`/`startsWith`, null checks, and `all`/`any`/`not` over them. A
//! relation, JSON-path, coalesce, vector or spatial filter is reported by
//! [`in_memory_columns`] so the caller can refuse it up front.

use std::cmp::Ordering;

use super::{FilterExpr, FilterOp};
use crate::{SqlColumnValue, SqlValue, values::FilterValue};

/// The columns `expr` reads, or the name of the first construct that cannot
/// be decided in memory.
pub fn in_memory_columns(expr: &FilterExpr) -> Result<Vec<&'static str>, &'static str> {
    let mut columns = Vec::new();
    collect(expr, &mut columns)?;
    columns.sort_unstable();
    columns.dedup();
    Ok(columns)
}

fn collect(expr: &FilterExpr, out: &mut Vec<&'static str>) -> Result<(), &'static str> {
    match expr {
        FilterExpr::Filter(filter) => {
            out.push(filter.column);
            Ok(())
        }
        FilterExpr::All(exprs) | FilterExpr::Any(exprs) => {
            exprs.iter().try_for_each(|expr| collect(expr, out))
        }
        FilterExpr::Not(expr) => collect(expr, out),
        FilterExpr::Relation(_) => Err("a relation filter"),
        FilterExpr::Coalesce(_) => Err("a coalesce filter"),
        FilterExpr::Json(_) => Err("a JSON path filter"),
        FilterExpr::VectorDistance(_) => Err("a vector distance filter"),
        #[cfg(feature = "postgis")]
        FilterExpr::Spatial(_) => Err("a spatial filter"),
    }
}

/// Whether the record with column values `values` satisfies `expr`, as a SQL
/// `WHERE` would decide. A column absent from `values`, or a construct
/// [`in_memory_columns`] rejects, does not match.
pub fn matches(expr: &FilterExpr, values: &[SqlColumnValue]) -> bool {
    truth(expr, values) == Some(true)
}

/// `Some(bool)` decided, `None` unknown (SQL `NULL`).
fn truth(expr: &FilterExpr, values: &[SqlColumnValue]) -> Option<bool> {
    match expr {
        FilterExpr::Filter(filter) => {
            let value = values.iter().find(|v| v.column == filter.column)?;
            compare_filter(filter.op, &value.value, &filter.value)
        }
        FilterExpr::All(exprs) => all(exprs.iter().map(|expr| truth(expr, values))),
        FilterExpr::Any(exprs) => any(exprs.iter().map(|expr| truth(expr, values))),
        FilterExpr::Not(expr) => truth(expr, values).map(|value| !value),
        _ => Some(false),
    }
}

/// SQL's three-valued `AND`: `FALSE` wins, then `NULL` (`None`).
fn all(truths: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(true);
    for truth in truths {
        match truth {
            Some(false) => return Some(false),
            None => result = None,
            Some(true) => {}
        }
    }
    result
}

/// SQL's three-valued `OR`: `TRUE` wins, then `NULL` (`None`).
fn any(truths: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(false);
    for truth in truths {
        match truth {
            Some(true) => return Some(true),
            None => result = None,
            Some(false) => {}
        }
    }
    result
}

fn compare_filter(op: FilterOp, left: &SqlValue, right: &FilterValue) -> Option<bool> {
    match op {
        FilterOp::IsNull => Some(is_null(left)),
        FilterOp::IsNotNull => Some(!is_null(left)),
        FilterOp::EqOrNull => {
            if is_null(left) {
                return Some(true);
            }
            single(right)
                .and_then(|right| order(left, right))
                .map(Ordering::is_eq)
        }
        FilterOp::In => {
            let FilterValue::Many(candidates) = right else {
                return Some(false);
            };
            // `x IN (a, b)` is `x = a OR x = b`.
            any(candidates
                .iter()
                .map(|candidate| order(left, candidate).map(Ordering::is_eq)))
        }
        FilterOp::Contains | FilterOp::StartsWith => match (left, single(right)?) {
            (SqlValue::String(text), SqlValue::String(pattern)) => Some(like(text, pattern)),
            _ => None,
        },
        FilterOp::Eq => order(left, single(right)?).map(Ordering::is_eq),
        FilterOp::Ne => order(left, single(right)?).map(Ordering::is_ne),
        FilterOp::Lt => order(left, single(right)?).map(Ordering::is_lt),
        FilterOp::Lte => order(left, single(right)?).map(Ordering::is_le),
        FilterOp::Gt => order(left, single(right)?).map(Ordering::is_gt),
        FilterOp::Gte => order(left, single(right)?).map(Ordering::is_ge),
    }
}

fn single(value: &FilterValue) -> Option<&SqlValue> {
    match value {
        FilterValue::Single(value) => Some(value),
        _ => None,
    }
}

fn is_null(value: &SqlValue) -> bool {
    match value {
        SqlValue::NullBool
        | SqlValue::NullInt
        | SqlValue::NullBigInt
        | SqlValue::NullFloat
        | SqlValue::NullString
        | SqlValue::NullBytes
        | SqlValue::NullUuid
        | SqlValue::NullDateTime
        | SqlValue::NullJson
        | SqlValue::NullDecimal
        | SqlValue::NullVector => true,
        #[cfg(feature = "postgis")]
        SqlValue::NullSpatial => true,
        _ => false,
    }
}

/// SQL ordering for the comparable scalar pairs; `None` (unknown) for a
/// `NULL` or a pair SQL would not compare.
fn order(left: &SqlValue, right: &SqlValue) -> Option<Ordering> {
    use SqlValue::*;
    match (left, right) {
        (Int(a) | BigInt(a), Int(b) | BigInt(b)) => Some(a.cmp(b)),
        (Float(a), Float(b)) => a.partial_cmp(b),
        (Float(a), Int(b) | BigInt(b)) => a.partial_cmp(&(*b as f64)),
        (Int(a) | BigInt(a), Float(b)) => (*a as f64).partial_cmp(b),
        (String(a), String(b)) => Some(a.cmp(b)),
        (Bool(a), Bool(b)) => Some(a.cmp(b)),
        (Uuid(a), Uuid(b)) => Some(a.cmp(b)),
        (DateTime(a), DateTime(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// SQL `LIKE`: `%` any run of characters, `_` exactly one.
fn like(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    // `reachable[j]`: the first `i` text characters can match the first `j`
    // pattern characters.
    let mut reachable = vec![false; pattern.len() + 1];
    reachable[0] = true;
    for j in 1..=pattern.len() {
        reachable[j] = reachable[j - 1] && pattern[j - 1] == '%';
    }
    for character in &text {
        let mut next = vec![false; pattern.len() + 1];
        for j in 1..=pattern.len() {
            next[j] = match pattern[j - 1] {
                '%' => next[j - 1] || reachable[j],
                '_' => reachable[j - 1],
                literal => reachable[j - 1] && literal == *character,
            };
        }
        reachable = next;
    }
    reachable[pattern.len()]
}

#[cfg(test)]
mod tests;
