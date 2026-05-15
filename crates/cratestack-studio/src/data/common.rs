//! Driver-agnostic helpers shared by every [`super::DataSource`] impl.
//!
//! The Postgres and SQLite sources have parallel shapes — page-limit
//! clamping, next-cursor extraction, sample-column synthesis — that
//! used to be duplicated verbatim. They live here now so each driver
//! can stay focused on dialect specifics.

use super::model_info::{ModelSqlInfo, json_value_to_cursor};
use super::{DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT, Row};

/// Clamp the caller-supplied limit into Studio's permitted range,
/// substituting the default when the caller didn't ask.
pub(crate) fn clamp_limit(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT)
}

/// Compute the next page's opaque cursor from the rows just returned.
///
/// Returns `Some(...)` only when the page is full (i.e. there may be
/// more rows). The cursor is the last row's primary-key value rendered
/// through [`json_value_to_cursor`] so it round-trips losslessly when
/// the next request re-binds it as text.
pub(crate) fn next_cursor(rows: &[Row], pk_field_name: &str, limit: u32) -> Option<String> {
    if rows.len() == limit as usize {
        rows.last()
            .and_then(|r| r.get(pk_field_name))
            .map(json_value_to_cursor)
    } else {
        None
    }
}

/// Column names for every scalar field on the model, in declaration
/// order. Used by `preview_sql` to synthesize a placeholder payload
/// when the caller didn't pass one.
pub(crate) fn sample_column_names(info: &ModelSqlInfo<'_>) -> Vec<String> {
    info.columns.iter().map(|c| c.column_name.clone()).collect()
}
