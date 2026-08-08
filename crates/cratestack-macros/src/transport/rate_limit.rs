//! `@no_rate_limit` participation logic shared by both transport
//! generators (`rest`'s `RouteTransportDescriptor` and
//! `op_descriptors`'s `OpDescriptor`), so REST and RPC schemas compute
//! `rate_limited_by_default` identically instead of drifting apart
//! (cratestack#474 — a fix that works for one transport and silently
//! no-ops for the other reproduces the bug in a narrower form).

use cratestack_core::Procedure;

/// Whether a procedure participates in rate limiting by default.
///
/// The parser already guarantees `@no_rate_limit` only appears on a
/// procedure when the enclosing schema declares `extension rate_limit
/// { }` (`validate_procedure_no_rate_limit_attribute` in
/// cratestack-parser), so codegen doesn't need to re-check
/// `declared_extensions` here — seeing the raw attribute string is
/// always meaningful.
pub(crate) fn procedure_rate_limited_by_default(procedure: &Procedure) -> bool {
    !procedure
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@no_rate_limit")
}
