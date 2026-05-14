//! Schema diff and migration generator for `.cstack`.
//!
//! This crate is the **authoring** side of the migration story. The
//! runner that applies SQL to a database lives in `cratestack-sqlx`
//! (forward-only, checksum-protected) and consumes the SQL produced
//! here identically to hand-written migrations.
//!
//! See ADR 0004 for the full design:
//! <https://cratestack.dev/internals/schema-diff-adr>.

mod convert;
mod diff;
pub mod emit;
mod error;
pub mod ir;
mod naming;
mod snapshot;

pub use diff::diff;
pub use emit::EmittedMigration;
pub use error::MigrateError;
pub use naming::{check_name, column_name, index_name_unique, table_name};
pub use snapshot::{
    SNAPSHOT_FORMAT_VERSION, Snapshot, read_or_empty, read_snapshot, write_snapshot,
};
