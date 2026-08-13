//! `CREATE EXTENSION` DDL for schema-level `.cstack` `extension <name>
//! { }` declarations that have a real Postgres extension behind them
//! (currently just `pgvector`'s `vector` extension — see
//! `docs/design/extensions.md` §6). Gated behind the `pgvector` Cargo
//! feature for the same reason `columns::render_vector_type` is: the
//! parser already guarantees an `EnsureExtension` op can only exist
//! for a schema that declared `extension pgvector { }`, so reaching
//! this without the feature enabled is an upstream invariant
//! violation, not a case to handle gracefully.

#[cfg(feature = "pgvector")]
use std::fmt::Write as _;

use crate::ir::EnsureExtension;

#[cfg(feature = "pgvector")]
pub(super) fn emit_ensure_extension(sql: &mut String, op: &EnsureExtension) {
    writeln!(sql, "CREATE EXTENSION IF NOT EXISTS {};", op.name).unwrap();
}

#[cfg(not(feature = "pgvector"))]
pub(super) fn emit_ensure_extension(_sql: &mut String, op: &EnsureExtension) {
    unreachable!(
        "EnsureExtension({:?}) reached the Postgres emitter without the `pgvector` Cargo \
         feature enabled on cratestack-migrate — this should be unreachable because only a \
         schema declaring `extension pgvector {{ }}` produces this op",
        op.name
    );
}
