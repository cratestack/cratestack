//! Keyset (cursor) pagination primitives, dialect-agnostic so the Postgres
//! and embedded backends share one definition
//! (`docs/design/cursor-pagination.md`).
//!
//! A keyset page is "the rows strictly after the last row of the previous
//! page, in the query's sort order". [`parse_keyset_sort`] resolves a
//! `sort` expression into ordered [`KeysetColumn`]s with the primary key
//! appended as a unique tiebreaker; [`keyset_after`] turns the last row's
//! values into an ordinary [`FilterExpr`] to `AND` into the query — so the
//! bound composes with filters and read policy rather than replacing them.
//! All ordering uses the framework's `NULLS LAST` default
//! ([`crate::NullOrder`]).

use cratestack_core::CratestackError;

use crate::FilterExpr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysetColumn {
    pub name: String,
    pub descending: bool,
    pub nullable: bool,
}

/// One comparison the caller must turn into a typed filter on `column`.
#[derive(Debug, Clone, Copy)]
pub enum KeysetLeaf<'a, V> {
    Eq(&'a V),
    Gt(&'a V),
    Lt(&'a V),
    IsNull,
}

/// Resolve `sort` (`"-createdAt,name"`; `None` = unsorted) into keyset
/// columns, appending `primary_key` ascending when absent so the order is
/// total. `nullable_of` returns `Some(is_nullable)` for a sortable scalar
/// column and `None` for anything else. Relation-path (`a.b`) terms,
/// unknown or duplicate columns and empty terms are `BadRequest`.
pub fn parse_keyset_sort(
    sort: Option<&str>,
    primary_key: &str,
    nullable_of: impl Fn(&str) -> Option<bool>,
) -> Result<Vec<KeysetColumn>, CratestackError> {
    let mut columns: Vec<KeysetColumn> = Vec::new();
    for raw in sort.into_iter().flat_map(|sort| sort.split(',')) {
        let raw = raw.trim();
        let (descending, name) = match raw.strip_prefix('-') {
            Some(name) => (true, name),
            None => (false, raw),
        };
        if name.is_empty() {
            return Err(CratestackError::BadRequest(
                "sort must not contain empty fields".to_owned(),
            ));
        }
        if name.contains('.') {
            return Err(CratestackError::BadRequest(format!(
                "cursor pagination does not support relation-path sort '{name}'"
            )));
        }
        let Some(nullable) = nullable_of(name) else {
            return Err(CratestackError::BadRequest(format!(
                "cursor pagination does not support sort field '{name}'"
            )));
        };
        if columns.iter().any(|column| column.name == name) {
            return Err(CratestackError::BadRequest(format!(
                "sort field '{name}' is repeated"
            )));
        }
        columns.push(KeysetColumn {
            name: name.to_owned(),
            descending,
            nullable,
        });
    }
    if !columns.iter().any(|column| column.name == primary_key) {
        columns.push(KeysetColumn {
            name: primary_key.to_owned(),
            descending: false,
            nullable: false,
        });
    }
    Ok(columns)
}

/// Canonical effective-order string (`"-createdAt,id"`) a cursor is bound to.
pub fn keyset_order_spec(columns: &[KeysetColumn]) -> String {
    columns
        .iter()
        .map(|column| {
            if column.descending {
                format!("-{}", column.name)
            } else {
                column.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Filter selecting the rows strictly after the row whose per-column
/// values are `values` (`None` = `NULL`), expanded as
/// `OR_i (c_0 = v_0 AND … AND c_{i-1} = v_{i-1} AND c_i AFTER v_i)`.
/// With `NULLS LAST`, a non-null `v_i` on a nullable column is also
/// followed by every `NULL`; a `NULL` `v_i` has nothing after it in that
/// column (only ties, handled by the next term). `leaf` builds the typed
/// comparison — the caller owns value parsing, so a bad value fails here
/// exactly as the same value in a `?col__gt=` filter would.
pub fn keyset_after<V, E>(
    columns: &[KeysetColumn],
    values: &[Option<V>],
    mut leaf: impl FnMut(&str, KeysetLeaf<'_, V>) -> Result<FilterExpr, E>,
) -> Result<FilterExpr, E>
where
    E: From<CratestackError>,
{
    if columns.len() != values.len() || columns.is_empty() {
        return Err(
            CratestackError::BadRequest("cursor does not match the sort order".to_owned()).into(),
        );
    }
    let mut terms = Vec::new();
    for (index, (column, value)) in columns.iter().zip(values).enumerate() {
        let Some(value) = value else { continue };
        let mut conjuncts = Vec::with_capacity(index + 1);
        for (prior, prior_value) in columns[..index].iter().zip(values) {
            conjuncts.push(match prior_value {
                Some(prior_value) => leaf(&prior.name, KeysetLeaf::Eq(prior_value))?,
                None => leaf(&prior.name, KeysetLeaf::IsNull)?,
            });
        }
        let strictly_after = if column.descending {
            leaf(&column.name, KeysetLeaf::Lt(value))?
        } else {
            leaf(&column.name, KeysetLeaf::Gt(value))?
        };
        conjuncts.push(if column.nullable {
            FilterExpr::any([strictly_after, leaf(&column.name, KeysetLeaf::IsNull)?])
        } else {
            strictly_after
        });
        terms.push(FilterExpr::all(conjuncts));
    }
    Ok(FilterExpr::any(terms))
}

#[cfg(test)]
#[path = "keyset_tests.rs"]
mod tests;
