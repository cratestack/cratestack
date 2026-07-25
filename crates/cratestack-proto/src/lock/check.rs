//! `--check` support for ticket #169's CLI: rebuild in memory and report
//! whether the committed lock would change, without writing anything.

use cratestack_core::Schema;

use super::build::build_lock;
use super::{PbLock, PbLockError};

pub fn lock_would_change(schema: &Schema, existing: &PbLock) -> Result<bool, PbLockError> {
    let rebuilt = build_lock(schema, Some(existing))?;
    Ok(&rebuilt != existing)
}
