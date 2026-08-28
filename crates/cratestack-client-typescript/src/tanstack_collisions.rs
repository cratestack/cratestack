//! Generation-time collision check for `--tanstack` (cratestack#802).
//!
//! The `--swr` analogue lives in `crate::swr::collisions` and is scoped to
//! that flag by construction — its module doc reasons explicitly about not
//! over-restricting "schemas that are never consumed with `--swr`".
//! Neither of its checks is reachable from the `--tanstack` path, so this
//! is a sibling rather than a shared helper, gated on its own flag for the
//! same reason.
//!
//! Same stance, set by #344 and reaffirmed by #777: this generator derives
//! identifiers the schema author never wrote, so it refuses a schema whose
//! derived names collide instead of pushing the failure downstream.
//! Decision spike #317 ruled out doing this in `cratestack-parser` — each
//! generator normalizes differently, so no single parser-level check can
//! cover them all without over-restricting schemas that never reach this
//! flag.
//!
//! # Why this one is sharper than `--swr`'s
//!
//! `--swr` emits model functions and procedure functions into *separate*
//! files that `src/swr/index.ts` barrel-`export *`s, so a collision there
//! is TS2308 at the barrel. `--tanstack` emits both families into the
//! **same** `src/react-query.ts`, so a collision is a same-file duplicate
//! declaration that no `export *` de-duplication can mask.
//!
//! Measured, not assumed: generating from
//! `tests/fixtures/tanstack_mutation_hook_collision.cstack` with this
//! check disabled produces two `export function useCreatePostMutation`
//! declarations in one file, and real `tsc` reports **TS2393** (duplicate
//! function implementation) and **TS2323** (cannot redeclare exported
//! variable) on both. #802 predicted TS2300 — a different
//! duplicate-identifier code, and not the one this path emits.
//!
//! # Both transports
//!
//! `templates/src/rest-react-query.ts.j2` and `rpc-react-query.ts.j2` emit
//! the same two families behind the same five `{% if model.allows_* %}`
//! gates, so one check covers both — unlike `--swr`, whose REST template
//! additionally emits `get{Model}WithResponse` (#610) and therefore needs
//! a transport argument. Checked against both templates rather than
//! assumed: a REST-only fix would leave the hazard live on RPC, which the
//! repo's transport-parity rule exists to prevent.

use std::collections::HashMap;

use crate::error::TypeScriptGeneratorError;
use crate::procedure_views::ProcedureView;
use crate::views::ModelApiView;

/// Refuses a schema whose `--tanstack` procedure hook name equals one of
/// the five derived model hook names.
///
/// A procedure contributes `use{hook_name}Query` when it is a query and
/// `use{hook_name}Mutation` when it is a mutation, where `hook_name` is
/// `to_pascal_case(&procedure.name)` (`crate::procedure_views`). Both
/// suffixes are checked because a procedure can collide through either:
/// `procedure post_list` produces `usePostListQuery`, byte-identical to
/// model `Post`'s list hook, while `procedure create_post` produces
/// `useCreatePostMutation`, byte-identical to its create hook. The
/// collision is not limited to already-PascalCase procedure names —
/// `post_list`, `postList` and `PostList` all normalize to `PostList`.
///
/// Fails loudly rather than disambiguating: either candidate rename — the
/// author's procedure or this generator's derived model hook — is the
/// schema author's call, not a default this generator gets to pick
/// silently.
pub(crate) fn reject_tanstack_hook_name_collisions(
    models: &[ModelApiView],
    query_procedures: &[ProcedureView],
    mutation_procedures: &[ProcedureView],
) -> Result<(), TypeScriptGeneratorError> {
    let mut derived: HashMap<String, (&str, &'static str)> = HashMap::new();
    for model in models {
        for (identifier, operation) in emitted_model_hook_names(model) {
            // First writer wins, matching `crate::swr::collisions`: two
            // models whose derived hook names collide with *each other*
            // is a separate defect this check does not own. Reporting the
            // first is enough to name a real conflicting declaration.
            derived
                .entry(identifier)
                .or_insert((model.name.as_str(), operation));
        }
    }

    for (procedures, suffix) in [
        (query_procedures, "Query"),
        (mutation_procedures, "Mutation"),
    ] {
        for procedure in procedures {
            let identifier = format!("use{}{suffix}", procedure.hook_name);
            if let Some(&(model, operation)) = derived.get(&identifier) {
                return Err(TypeScriptGeneratorError::TanstackHookNameCollision {
                    procedure: procedure.name.clone(),
                    identifier,
                    model: model.to_owned(),
                    operation,
                });
            }
        }
    }
    Ok(())
}

/// The subset of a model's five derived hook names that this model
/// actually exports, gated exactly as `templates/src/{rest,rpc}-react-query.ts.j2`
/// gate them. A hook suppressed by `@@internal` (#743) or by a missing
/// `create` rule is never emitted, so it cannot collide — rejecting a
/// schema for a hook that does not exist would be over-rejection, the
/// specific risk #802 flags.
fn emitted_model_hook_names(model: &ModelApiView) -> Vec<(String, &'static str)> {
    let mut names = Vec::new();
    if model.allows_list {
        names.push((format!("use{}ListQuery", model.name), "list"));
    }
    if model.allows_get {
        names.push((format!("use{}Query", model.name), "get"));
    }
    if model.allows_create {
        names.push((format!("useCreate{}Mutation", model.name), "create"));
    }
    if model.allows_update {
        names.push((format!("useUpdate{}Mutation", model.name), "update"));
    }
    if model.allows_delete {
        names.push((format!("useDelete{}Mutation", model.name), "delete"));
    }
    names
}
