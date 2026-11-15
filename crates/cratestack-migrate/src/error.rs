use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("failed to read snapshot file {path}: {source}")]
    SnapshotRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write snapshot file {path}: {source}")]
    SnapshotWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse snapshot file {path}: {source}")]
    SnapshotParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error(
        "snapshot file {path} has unsupported format version {found} \
         (this build understands version {expected}); regenerate it with a \
         compatible cratestack-migrate"
    )]
    SnapshotFormatVersion {
        path: PathBuf,
        found: u32,
        expected: u32,
    },

    #[error("failed to serialize snapshot: {0}")]
    SnapshotSerialize(#[source] serde_json::Error),

    /// Surfaces a failure computing [`crate::projections_checksum`]
    /// (issue #205). In practice unreachable for any `Projections`
    /// value this crate itself produces — no non-string map keys, no
    /// floats — but the input can also come from a hand-edited
    /// snapshot deserialized off disk, so this stays a real error
    /// rather than a panic.
    #[error("failed to serialize projections for checksum: {0}")]
    ChecksumSerialize(#[source] serde_json::Error),

    /// An existing table's primary key changed shape — a column was
    /// added, removed, or reordered (issue #536). Deliberately a
    /// refusal, not an `Op`: a correct migration needs constraint
    /// drop/recreate ordering, dependent foreign keys, and a
    /// data-safety story for a populated table, none of which the
    /// diff engine has today. Silently emitting nothing (the previous
    /// behavior) is worse than refusing loudly — see
    /// `crate::diff::primary_key` for the detection and the full
    /// rationale for refusing rather than emitting.
    #[error(
        "table `{table}`: primary key changed from {prev} to {next} — cratestack-migrate does \
         not generate a migration for a primary-key change on an existing table (it needs \
         constraint drop/recreate ordering, dependent foreign keys, and a data-safety plan for \
         existing rows that this engine does not have). Revert the primary-key change, or write \
         the ALTER TABLE migration for `{table}` by hand."
    )]
    PrimaryKeyChanged {
        table: String,
        prev: String,
        next: String,
    },

    /// An ordinal enum's history was rewritten: a variant renumbered, an
    /// ordinal dropped without being reserved, or a used or reserved ordinal
    /// handed to another variant (`docs/design/enum-ordinals.md` §3.2).
    /// Stored rows mean what their number meant when they were written, so
    /// the diff refuses rather than emits.
    #[error(
        "enum `{enum_name}`: {reason}. Ordinals are permanent: stored rows keep the meaning their number had when written."
    )]
    EnumOrdinalHistory { enum_name: String, reason: String },

    /// A model's table is stored under another name (its `@@map` was added,
    /// changed or removed) without a `@@rename`. Matching is by name, so this
    /// would otherwise drop the table and create an empty one (cratestack#1375).
    #[error(
        "model `{model}`: its table changed from `{from}` to `{to}` (`@@map`), and a changed \
         name would drop `{from}` and create an empty `{to}`. To rename the table, add \
         `@@rename(from = \"{from}\")` to the model; to keep it, restore the old `@@map`. To \
         adopt a database that already has `{to}`, run `cratestack migrate baseline` after \
         writing the `@@map`."
    )]
    TableStorageNameChanged {
        model: String,
        from: String,
        to: String,
    },

    /// A field's column is stored under another name (its `@map` was added,
    /// changed or removed) without a `@rename`; same reasoning as
    /// [`Self::TableStorageNameChanged`].
    #[error(
        "table `{table}` (model `{model}`): field `{field}` is stored in column `{to}`, not \
         `{from}` (`@map`), and a changed name would drop `{from}` and add an empty `{to}`. To \
         rename the column, add `@rename(from = \"{from}\")` to `{field}`; to keep it, restore \
         the old `@map`. To adopt a database that already has `{to}`, run `cratestack migrate \
         baseline` after writing the `@map`."
    )]
    ColumnStorageNameChanged {
        table: String,
        model: String,
        field: String,
        from: String,
        to: String,
    },

    /// A mapped model's table is new, while a table the previous snapshot
    /// holds without saying which model it stores is gone. The snapshot came
    /// from `migrate baseline` (or predates `model`), so the diff cannot tell
    /// a changed `@@map` from an unrelated drop and create (cratestack#1375).
    #[error(
        "model `{model}` is stored in table `{to}` (`@@map`), which the previous snapshot does \
         not have, and a table the snapshot has is gone from the schema. That snapshot does \
         not record which model each table stores, so this cannot be told from a `@@map` \
         change, and a changed name would drop the old table and create an empty `{to}`. If it \
         is one of the tables that is gone, add {renames} to the model; to keep it, \
         restore the old `@@map`; to drop and create deliberately, do it in two migrations."
    )]
    TableStorageNameUnrecorded {
        model: String,
        to: String,
        renames: String,
    },

    /// The column counterpart of [`Self::TableStorageNameUnrecorded`].
    #[error(
        "table `{table}` (model `{model}`): field `{field}` is stored in column `{to}` (`@map`), \
         which the previous snapshot does not have, and a column the snapshot has is gone from \
         the schema. That snapshot does not record which field each column stores, so this \
         cannot be told from a `@map` change, and a changed name would drop the old column and \
         add an empty `{to}`. If it is a column that is gone, add {renames} to \
         `{field}`; to keep it, restore the old `@map`; to drop and add deliberately, do it in \
         two migrations."
    )]
    ColumnStorageNameUnrecorded {
        table: String,
        model: String,
        field: String,
        to: String,
        renames: String,
    },
}
