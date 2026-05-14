//! SQLite SQL emitter placeholder.
//!
//! Slice 4 fills this in. Slice 3 only exists to introduce the
//! `EmittedMigration` shape and the Postgres emitter; SQLite's quirks
//! around `ALTER TABLE` (no `DROP COLUMN` until 3.35, no in-place
//! type changes, table rebuild for many alters) deserve their own
//! slice.
