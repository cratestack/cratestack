//! Schema diff and migration generator for `.cstack`.
//!
//! This crate is the **authoring** side of the migration story. The
//! runner that applies SQL to a database lives in `cratestack-sqlx`
//! (forward-only, checksum-protected) and consumes the SQL produced
//! here identically to hand-written migrations.
//!
//! See ADR 0004 for the full design:
//! <https://cratestack.dev/internals/schema-diff-adr>.

mod error;
mod snapshot;

pub use error::MigrateError;
pub use snapshot::{Snapshot, SNAPSHOT_FORMAT_VERSION, read_snapshot, write_snapshot};
