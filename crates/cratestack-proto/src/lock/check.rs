//! `--check` support for ticket #169's CLI: rebuild in memory and report
//! whether the committed lock would change, without writing anything.

use std::collections::BTreeMap;

use cratestack_core::{Field, Schema};

use super::build::build_lock;
use super::{PbLock, PbLockError};

pub fn lock_would_change(
    schema: &Schema,
    existing: &PbLock,
    extra_messages: &BTreeMap<String, Vec<Field>>,
) -> Result<bool, PbLockError> {
    let rebuilt = build_lock(schema, Some(existing), extra_messages)?;
    Ok(&rebuilt != existing)
}
