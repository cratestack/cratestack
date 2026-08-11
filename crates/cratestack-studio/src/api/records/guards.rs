//! Pre-flight checks for mutation handlers ([`super::writes`]) — run
//! before the data source is touched, so a rejected write never issues
//! any SQL.

use cratestack_core::Model;

use crate::api::ApiError;
use crate::config::TargetMode;
use crate::workspace::LoadedTarget;

/// Reject mutation requests against read-only targets at the earliest
/// point — before we touch the data source.
pub(in crate::api::records) fn require_writable(target: &LoadedTarget) -> Result<(), ApiError> {
    if matches!(target.mode, TargetMode::Rw) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// Refuse to write a `@version`/`@@emit` model straight to SQL on a
/// `[target.db]` target unless the target opted in via
/// `allow_unsafe_writes` (cratestack#507).
///
/// A `[target.db]` connection talks raw SQL, not the descriptor path the
/// generated server runs: it never bumps `@version` columns and never
/// writes `cratestack_event_outbox` rows for `@@emit`-annotated models.
/// Both omissions previously landed silently — a `200` with no signal
/// that optimistic concurrency or the event outbox didn't apply. This
/// makes the bypass something an operator chooses per target rather
/// than discovers after the fact. Targets reached only through
/// `[target.api]` are unaffected: those writes go through the deployed
/// service's generated routes, which already apply `@version`/`@@emit`
/// (and `@@allow`) themselves.
///
/// Returns `Ok(true)` when the write is allowed *because* it bypassed
/// `@version`/`@@emit` via `allow_unsafe_writes` — the caller threads
/// this into the audit entry (see [`crate::audit::AuditEntry::unsafe_write`])
/// so `/api/audit` and the JSONL sidecar can tell a bypass write apart
/// from an ordinary one after the fact, rather than only at the moment
/// an operator flips the config flag. `Ok(false)` covers every other
/// allowed case: no `[target.db]`, or a model with neither annotation.
pub(in crate::api::records) fn require_safe_write(
    target: &LoadedTarget,
    model_decl: &Model,
) -> Result<bool, ApiError> {
    if !target.has_db {
        return Ok(false);
    }

    let mut annotations = Vec::new();
    if model_decl
        .fields
        .iter()
        .any(|f| f.attributes.iter().any(|a| a.raw == "@version"))
    {
        annotations.push("@version");
    }
    if model_decl
        .attributes
        .iter()
        .any(|a| a.raw.starts_with("@@emit("))
    {
        annotations.push("@@emit(...)");
    }

    if annotations.is_empty() {
        return Ok(false);
    }

    if !target.allow_unsafe_db_writes {
        return Err(ApiError::UnsafeDbWrite {
            target: target.key.clone(),
            model: model_decl.name.clone(),
            annotations: annotations.join(" and "),
        });
    }

    // The opt-in fired: this write is about to skip `@version`/`@@emit`
    // for real. A bare early return here would reproduce the exact
    // silence cratestack#507 reported, one config line upstream — so
    // this is loud both in the logs (here) and durably in the audit
    // trail (the `true` the caller records on the entry).
    tracing::warn!(
        target = %target.key,
        model = %model_decl.name,
        annotations = %annotations.join(" and "),
        "allow_unsafe_writes is set on [target.db]; writing straight to SQL and skipping {}",
        annotations.join(" and "),
    );
    Ok(true)
}

#[cfg(test)]
mod tests;
