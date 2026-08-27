//! Pre-flight checks for mutation handlers ([`super::writes`]) — run
//! before the data source is touched, so a rejected write never issues
//! any SQL.

use cratestack_core::{Model, ModelEventKind};

use crate::api::ApiError;
use crate::config::TargetMode;
use crate::data::model_info::emitted_events;
use crate::workspace::LoadedTarget;

/// Reject mutation requests against read-only targets at the earliest
/// point — before we touch the data source.
///
/// [`TargetMode::Rw`] is the whole check here on purpose, and the
/// omission is the documented decision rather than a gap: no
/// schema-declared write constraint is consulted anywhere in this
/// module — not `@@allow`/`@@deny`, not `@@internal(...)` route
/// suppression — because a `[target.db]` target is a direct SQL
/// connection that sits beneath the schema the way `psql` does
/// (cratestack#744, option 3: document it, don't enforce it; option 1,
/// gating `@@internal` here while `@@allow` stayed unenforced, was
/// considered and rejected as the arbitrary half). `@@internal`'s shared
/// predicate `cratestack_core::model_internal_actions` exists and is
/// deliberately not called from this crate. A `[target.api]`-only target
/// gets both constraints for free from the deployed service, since its
/// writes are ordinary HTTP calls against the generated routes. The
/// crate's top-level rustdoc ("Granting `rw`") and
/// `docs/design/route-suppression.md` §8b are what promise today's
/// behavior to operators; reversing this means editing both.
pub(in crate::api::records) fn require_writable(target: &LoadedTarget) -> Result<(), ApiError> {
    if matches!(target.mode, TargetMode::Rw) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// How a `[target.db]` write to one model should be carried out
/// (cratestack#507's write-side fix, in two stages):
///
/// - PR #516 made the *bypass itself* a choice (`allow_unsafe_writes`)
///   instead of a silent default.
/// - This ("option 3") makes the bypass unnecessary wherever it's
///   structurally possible: `@version` bumping is always routable (every
///   `DataSource` backend can run `version = version + 1`), and
///   `@@emit`'s `cratestack_event_outbox` row is routable on any backend
///   whose [`crate::data::DataSource::supports_event_outbox`] returns
///   `true` (currently just Postgres — see that method's doc comment for
///   why SQLite has no equivalent to route to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::api::records) enum WriteMode {
    /// No `@version`/`@@emit` on this model, or no `[target.db]` at all
    /// — nothing to route. Handlers call the plain
    /// `create`/`update`/`delete`.
    Plain,
    /// Every annotation the model declares can be applied for real on
    /// this backend. Handlers call `create_routed`/`update_routed`/
    /// `delete_routed`. Never an audit bypass — this is the correct
    /// behavior, not a workaround.
    Routed,
    /// The model declares `@@emit(...)` and this backend has no event
    /// outbox to route it through, but the target opted into
    /// `allow_unsafe_writes`. Handlers call the plain, unrouted
    /// `create`/`update`/`delete` — the same raw-SQL bypass PR #516
    /// introduced the opt-in for. Always an audit bypass.
    Bypassed,
}

/// Decide how a write to `model_decl` on `target` should be carried out,
/// refusing it outright when it can't be routed and the target hasn't
/// opted into the bypass. See [`WriteMode`] for the three outcomes.
pub(in crate::api::records) fn require_write_mode(
    target: &LoadedTarget,
    model_decl: &Model,
) -> Result<WriteMode, ApiError> {
    if !target.has_db {
        return Ok(WriteMode::Plain);
    }

    let versioned = model_decl
        .fields
        .iter()
        .any(|f| f.attributes.iter().any(|a| a.raw == "@version"));
    let emitted = emitted_events(model_decl);

    if !versioned && emitted.is_empty() {
        return Ok(WriteMode::Plain);
    }

    // `@version` alone is always routable, on every backend, so it never
    // blocks a write on its own. Only an `@@emit` this backend can't
    // honor for real does.
    if !emitted.is_empty() && !target.source.supports_event_outbox() {
        if !target.allow_unsafe_db_writes {
            return Err(ApiError::UnsafeDbWrite {
                // clone: error outlives this call, cannot borrow from `target`
                target: target.key.clone(),
                // clone: error outlives this call, cannot borrow from `model_decl`
                model: model_decl.name.clone(),
                annotations: format!("@@emit({})", format_kinds(&emitted)),
            });
        }
        // The opt-in fired: this write is about to skip `@@emit` for
        // real (and, since `Bypassed` calls the unrouted path
        // wholesale, `@version` too — see `WriteMode::Bypassed`'s doc
        // comment). A bare early return here would reproduce the exact
        // silence cratestack#507 reported, one config line upstream — so
        // this is loud both in the logs (here) and durably in the audit
        // trail (the caller records `unsafe_write: true` on the entry).
        tracing::warn!(
            target = %target.key,
            model = %model_decl.name,
            annotations = %format!("@@emit({})", format_kinds(&emitted)),
            "allow_unsafe_writes is set on [target.db]; writing straight to SQL and skipping @@emit \
             (and @version, since the bypass is all-or-nothing)",
        );
        return Ok(WriteMode::Bypassed);
    }

    Ok(WriteMode::Routed)
}

fn format_kinds(kinds: &[ModelEventKind]) -> String {
    kinds
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
