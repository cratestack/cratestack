//! Shared batch primitives: size cap, duplicate-key rejection, per-item
//! error/result envelopes, summary aggregation.

use std::hash::Hash;

use cratestack_core::{
    BATCH_MAX_ITEMS, BatchItemError, BatchItemResult, BatchItemStatus, BatchResponse, BatchSummary,
    find_duplicate_position,
};
use cratestack_sql::{SqlValue, find_duplicate_sql_value};

use crate::RusqliteError;

pub(super) fn validate_batch_size(len: usize) -> Result<(), RusqliteError> {
    if len > BATCH_MAX_ITEMS {
        return Err(RusqliteError::BatchTooLarge {
            actual: len,
            maximum: BATCH_MAX_ITEMS,
        });
    }
    Ok(())
}

pub(super) fn reject_duplicate_pks<K: Eq + Hash + Clone>(keys: &[K]) -> Result<(), RusqliteError> {
    if let Some((first, dup)) = find_duplicate_position(keys.iter().cloned()) {
        return Err(RusqliteError::DuplicateBatchKey {
            first,
            duplicate: dup,
        });
    }
    Ok(())
}

pub(super) fn reject_duplicate_sql_values(values: &[SqlValue]) -> Result<(), RusqliteError> {
    if let Some((first, dup)) = find_duplicate_sql_value(values) {
        return Err(RusqliteError::DuplicateBatchKey {
            first,
            duplicate: dup,
        });
    }
    Ok(())
}

/// Build a per-item error envelope from a rusqlite error. Recognises any
/// constraint violation as `CONFLICT` so the wire shape matches the server
/// side's projection for unique-key, PK, and CHECK failures alike. SQLite
/// uses different extended codes for SQLITE_CONSTRAINT_UNIQUE (2067) and
/// SQLITE_CONSTRAINT_PRIMARYKEY (1555); matching on the broad code lets
/// either land as `CONFLICT` without us having to enumerate every subtype.
pub(super) fn item_error(error: rusqlite::Error) -> BatchItemError {
    let is_constraint_violation = matches!(
        &error,
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation
    );
    BatchItemError {
        code: if is_constraint_violation {
            "CONFLICT".to_owned()
        } else {
            "DATABASE_ERROR".to_owned()
        },
        message: error.to_string(),
    }
}

pub(super) fn ok_item<T>(index: usize, value: T) -> BatchItemResult<T> {
    BatchItemResult {
        index,
        status: BatchItemStatus::Ok { value },
    }
}

pub(super) fn err_item<T>(index: usize, error: BatchItemError) -> BatchItemResult<T> {
    BatchItemResult {
        index,
        status: BatchItemStatus::Error { error },
    }
}

pub(super) fn finalize<T>(results: Vec<BatchItemResult<T>>) -> BatchResponse<T> {
    let total = results.len();
    let ok = results
        .iter()
        .filter(|r| matches!(r.status, BatchItemStatus::Ok { .. }))
        .count();
    BatchResponse {
        results,
        summary: BatchSummary {
            total,
            ok,
            err: total - ok,
        },
    }
}
