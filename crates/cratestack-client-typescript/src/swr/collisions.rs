//! Generation-time collision checks for `--swr`, run once per schema
//! before any file is rendered. Split out of `super` (cratestack#777)
//! once a second check arrived and the two together would have pushed
//! `mod.rs` past this repo's ~200-LoC convention.
//!
//! Both checks share one stance, set by #344 and reaffirmed by #777:
//! `--swr` derives identifiers the schema author never wrote, so this
//! generator — the one that owns the naming scheme — refuses a schema
//! whose derived names collide, rather than pushing the failure
//! downstream. Decision spike #317 ruled out doing this in
//! `cratestack-parser`: each generator normalizes differently, so no
//! single parser-level check can cover them all without over-restricting
//! schemas that are never consumed with `--swr`.

use std::collections::HashMap;

use cratestack_core::TransportStyle;

use crate::error::TypeScriptGeneratorError;
use crate::procedure_views::ProcedureView;
use crate::swr::views::SwrModelFileContext;

/// Issue #344: `PerModel` output paths (`src/swr/models/{{ file_stem }}.ts`
/// and its `.hooks.ts` sibling) are keyed solely by
/// `SwrModelFileContext::file_stem`, which `crate::naming::to_kebab_case`
/// derives from the model's schema name through the same lossy tokenizer
/// every other derived-name helper in this crate shares. Two distinct
/// models that tokenize identically (`UserGroup`/`User_Group`, see
/// `tests/fixtures/swr_key_collision.cstack`) would otherwise silently
/// clobber each other's generated file with no error — this check runs
/// once, before any file is rendered, so a collision is refused up front
/// rather than discovered by diffing generator output.
pub(crate) fn reject_model_file_name_collisions(
    model_contexts: &[SwrModelFileContext],
) -> Result<(), TypeScriptGeneratorError> {
    let mut seen_by_file_stem: HashMap<&str, &str> = HashMap::new();
    for model_context in model_contexts {
        let file_stem = model_context.file_stem.as_str();
        let model_name = model_context.model.name.as_str();
        if let Some(&first) = seen_by_file_stem.get(file_stem) {
            return Err(TypeScriptGeneratorError::SwrModelFileNameCollision {
                first: first.to_owned(),
                second: model_name.to_owned(),
                file_stem: file_stem.to_owned(),
            });
        }
        seen_by_file_stem.insert(file_stem, model_name);
    }
    Ok(())
}

/// Issue #777: `src/swr/index.ts` barrel-`export *`s from every
/// `./models/<model>.js` *and* from `./procedures.js`, and both sides
/// emit plain free functions — unlike the default layout, whose model
/// operations are methods on per-model client classes and so cannot
/// collide with a top-level procedure function at all. A procedure whose
/// `to_camel_case` form equals one of the five derived model function
/// names (`list{Models}`/`get{Model}`/`create{Model}`/`update{Model}`/
/// `delete{Model}`) therefore produces two `export *`-visible bindings of
/// the same name, which `tsc` rejects with **TS2308** — a broken package,
/// discovered only at the consumer's build. Note the collision is not
/// limited to already-camelCase procedure names: `procedure list_posts`
/// tokenizes to the same `listPosts`.
///
/// Refused here rather than in the parser for #317's reason (see this
/// module's own doc comment), and by failing loudly rather than
/// disambiguating because either candidate rename — the schema author's
/// procedure or this generator's derived model function — is the schema
/// author's call, not a default this generator gets to pick silently.
pub(crate) fn reject_procedure_name_collisions(
    procedures: &[ProcedureView],
    model_contexts: &[SwrModelFileContext],
    transport: TransportStyle,
) -> Result<(), TypeScriptGeneratorError> {
    let mut derived: HashMap<&str, (&str, &'static str)> = HashMap::new();
    for model_context in model_contexts {
        let model = model_context.model.name.as_str();
        for (identifier, operation) in emitted_model_fn_names(model_context, transport) {
            // First writer wins: two models whose derived function names
            // collide with each other is a separate defect this check
            // does not own (and `list{pluralize(..)}` makes it reachable
            // — `Post`/`Posts` both list as `listPosts`). Reporting the
            // first is enough to name a real conflicting declaration.
            derived.entry(identifier).or_insert((model, operation));
        }
    }

    for procedure in procedures {
        if let Some(&(model, operation)) = derived.get(procedure.method_name.as_str()) {
            return Err(TypeScriptGeneratorError::SwrProcedureNameCollision {
                procedure: procedure.name.clone(),
                identifier: procedure.method_name.clone(),
                model: model.to_owned(),
                operation,
            });
        }
    }
    Ok(())
}

/// The subset of a model's five derived function names that this model
/// actually exports, gated exactly as `templates/src/swr/models-{rest,
/// rpc}.ts.j2` gate them — a name suppressed by `@@internal` (#743) or by
/// a missing `create` rule is never emitted, so it cannot collide.
/// `get{Model}WithResponse` (#610) is REST-only: the RPC template never
/// renders it, so an RPC schema must not be rejected for it.
fn emitted_model_fn_names(
    model_context: &SwrModelFileContext,
    transport: TransportStyle,
) -> Vec<(&str, &'static str)> {
    let mut names = Vec::new();
    if model_context.model.allows_list {
        names.push((model_context.list_fn.as_str(), "list"));
    }
    if model_context.model.allows_get {
        names.push((model_context.get_fn.as_str(), "get"));
        if matches!(transport, TransportStyle::Rest) {
            names.push((
                model_context.get_with_response_fn.as_str(),
                "get (with response)",
            ));
        }
    }
    if model_context.create_input.is_some() {
        names.push((model_context.create_fn.as_str(), "create"));
    }
    if model_context.update_input.is_some() {
        names.push((model_context.update_fn.as_str(), "update"));
    }
    if model_context.model.allows_delete {
        names.push((model_context.delete_fn.as_str(), "delete"));
    }
    names
}
