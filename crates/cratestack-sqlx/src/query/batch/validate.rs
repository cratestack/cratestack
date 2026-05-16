//! Outer-batch guards — size cap + duplicate-key rejection. All three
//! return `CoolError::Validation` so the wrapping `run` fn returns
//! before opening a transaction.

use std::hash::Hash;

use cratestack_core::{BATCH_MAX_ITEMS, CoolError, find_duplicate_position};

use crate::SqlValue;

pub(super) fn validate_batch_size(len: usize) -> Result<(), CoolError> {
    if len > BATCH_MAX_ITEMS {
        return Err(CoolError::Validation(format!(
            "batch size {len} exceeds maximum of {BATCH_MAX_ITEMS}",
        )));
    }
    Ok(())
}

pub(super) fn reject_duplicate_pks<K: Eq + Hash + Clone>(keys: &[K]) -> Result<(), CoolError> {
    if let Some((first, dup)) = find_duplicate_position(keys.iter().cloned()) {
        return Err(CoolError::Validation(format!(
            "duplicate primary key in batch at positions {first} and {dup}",
        )));
    }
    Ok(())
}

pub(super) fn reject_duplicate_sql_values(values: &[SqlValue]) -> Result<(), CoolError> {
    if let Some((first, dup)) = cratestack_sql::find_duplicate_sql_value(values) {
        return Err(CoolError::Validation(format!(
            "duplicate primary key in batch at positions {first} and {dup}",
        )));
    }
    Ok(())
}
