//! `@no_idempotency` participation logic shared by both transport
//! generators (`rest`'s `RouteTransportDescriptor` and
//! `op_descriptors`'s `OpDescriptor`), so REST and RPC schemas compute
//! `idempotent_by_default` identically instead of drifting apart.
//!
//! Structurally a twin of `super::rate_limit`, and for the same reason it
//! gives: a fix that works for one transport and silently no-ops for the
//! other reproduces cratestack#474's bug in a narrower form. The RPC half
//! of this predicate already existed, inline in `op_descriptors.rs`, when
//! REST had none — which is exactly the asymmetry that made the sharing
//! worth a module rather than a copied `matches!`.
//!
//! **One deliberate difference from `rate_limit`.** `@no_rate_limit` is
//! only meaningful in a schema that declares `extension rate_limit { }`,
//! and the parser enforces that before codegen ever sees it.
//! `@no_idempotency` is **not** gated on any `extension` block: it parses
//! and validates on any procedure today, and this helper honours it
//! wherever it appears. Whether idempotency should acquire an `extension
//! idempotency { }` of its own is an epic-level question (#875), not a
//! slice-1 one — adding the gate here would be inventing grammar the
//! parser does not have.

use cratestack_core::{Procedure, ProcedureKind};

/// Whether a procedure is safe to retry without an idempotency key, and
/// therefore takes no reservation.
///
/// Two disjoint reasons, deliberately collapsed into one flag (ADR 0015
/// (d) — no consumer has needed to tell "inherently safe" from "opted
/// out" apart, and an unread flag is a worse artefact than no flag):
///
/// - a `query procedure` is a read, and reads have always been `true`
///   here — this half is the predicate that used to live inline in
///   `op_descriptors.rs`, moved verbatim;
/// - a mutation the author marked `@no_idempotency` is opting out.
pub(crate) fn procedure_idempotent_by_default(procedure: &Procedure) -> bool {
    matches!(procedure.kind, ProcedureKind::Query)
        || procedure
            .attributes
            .iter()
            .any(|attribute| attribute.raw == "@no_idempotency")
}
