//! `CREATE EXTENSION` DDL for schema-level `.cstack` `extension <name>
//! { }` declarations that have a real Postgres extension behind them —
//! `pgvector`'s `vector` and PostGIS's `postgis` (see
//! `docs/design/extensions.md` §6/§6b). Gated behind those Cargo
//! features for the same reason `columns::render_vector_type` and
//! `columns::render_spatial_type` are: the parser already guarantees an
//! `EnsureExtension` op can only exist for a schema that declared the
//! matching `extension` block, so reaching this without the feature
//! enabled is an upstream invariant violation, not a case to handle
//! gracefully.
//!
//! The emission itself is extension-agnostic (it just writes
//! `op.name`), so the gate is `any(...)` over the extension features
//! rather than one arm per extension — with a per-extension gate,
//! building with `postgis` but not `pgvector` would panic on a
//! perfectly valid PostGIS schema.

#[cfg(any(feature = "pgvector", feature = "postgis"))]
use std::fmt::Write as _;

use crate::ir::EnsureExtension;

#[cfg(any(feature = "pgvector", feature = "postgis"))]
pub(super) fn emit_ensure_extension(sql: &mut String, op: &EnsureExtension) {
    writeln!(sql, "CREATE EXTENSION IF NOT EXISTS {};", op.name).unwrap();
}

#[cfg(not(any(feature = "pgvector", feature = "postgis")))]
pub(super) fn emit_ensure_extension(_sql: &mut String, op: &EnsureExtension) {
    unreachable!(
        "EnsureExtension({:?}) reached the Postgres emitter with neither the `pgvector` nor \
         the `postgis` Cargo feature enabled on cratestack-migrate — this should be \
         unreachable because only a schema declaring the matching `extension` block produces \
         this op",
        op.name
    );
}
