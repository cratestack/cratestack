use cratestack_core::{Field, Schema};
use serde::Serialize;

use crate::config::TypeScriptGeneratorConfig;
use crate::decimal::{DecimalShapeView, build_decimal_shapes};
use crate::error::TypeScriptGeneratorError;
use crate::find_many_views::{
    build_find_many_interface, build_order_by_clause_interface, build_sort_field_view,
    build_where_interface,
};
use crate::naming::{occupied_type_names, package_class_stem, to_pascal_case};
use crate::package_deps::{DependencyEntry, dev_dependencies_for, peer_dependencies_for};
use crate::procedure_views::{ProcedureView, build_procedure};
use crate::refine::{RefineResourceView, build_refine_resources, refine_resource_map_type};
use crate::types::{
    enum_name_set, is_computed_field, is_generated_on_create, is_primary_key, model_allows_create,
    model_name_set, scalar_model_fields, version_field, visible_model_fields,
};
use crate::views::{
    EnumView, InterfaceKind, InterfaceView, ModelApiView, build_computed_params_interface,
    build_enum_view, build_interface, build_model_api, disambiguate_model_api_keys,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TemplateContext {
    package_name: String,
    client_class_name: String,
    base_path: String,
    /// Issue #178, REST/RPC only — see `TypeScriptGeneratorConfig::schema_sha256`'s
    /// doc comment for the scope decision. Baked into `runtime.ts` as
    /// `SCHEMA_SHA256`; empty when the CLI wasn't given a schema fingerprint.
    schema_sha256: String,
    enums: Vec<EnumView>,
    interfaces: Vec<InterfaceView>,
    models: Vec<ModelApiView>,
    procedures: Vec<ProcedureView>,
    query_procedures: Vec<ProcedureView>,
    mutation_procedures: Vec<ProcedureView>,
    /// One row per model/`type` in the schema — the generated
    /// `decimalShapes` registry `models.ts.j2` renders and every decode
    /// call site looks a name up in (`crate::decimal`'s module doc has the
    /// full rationale; cratestack#499 review remediation).
    decimal_shapes: Vec<DecimalShapeView>,
    /// Issue #571 (`--refine`). Mirrors
    /// `TypeScriptGeneratorConfig::refine`, and is read by the two
    /// *unconditional* templates that also change under the flag —
    /// `package.json.j2` (adds the `@cratestack/refine` peer/dev
    /// dependency) and `rest-index.ts.j2` (re-exports `./refine.js`).
    /// `src/refine.ts` itself is gated by spec selection, not by this
    /// field, so it is simply absent from a default run.
    refine: bool,
    /// The semver range the generated `package.json` pins
    /// `@cratestack/refine` to under `--refine`. Derived from this
    /// crate's own `CARGO_PKG_VERSION`, which `just bump` moves in
    /// lockstep with the npm package — a generated client and the
    /// `@cratestack/refine` it was generated against are the same
    /// release, and a caret range on a `0.x` version resolves to that
    /// minor line only. Empty when `refine` is off, where no template
    /// reads it.
    refine_version_requirement: String,
    /// One entry per model, empty unless `refine` is set. See
    /// `crate::refine`.
    refine_resources: Vec<RefineResourceView>,
    /// The `@cratestack/refine` type `refine.ts.j2`'s
    /// `cratestackRefineResources()` is typed to return — see
    /// `crate::refine::refine_resource_map_type`. Empty when `refine` is
    /// off, where `refine.ts` isn't rendered at all.
    refine_resource_map_type: String,
    /// Issue #591 (`--swr`). Mirrors `TypeScriptGeneratorConfig::swr`, and
    /// is read by `package.json.j2` — the only unconditional template that
    /// changes under the flag, gaining the `"./swr"` `exports` subpaths
    /// and the `swr`/`react` peer/dev dependencies. `src/swr/**` itself is
    /// a wholly separate file set (`crate::swr::generate`), not gated by
    /// this field.
    swr: bool,
    /// Issue #617 (`--tanstack`). Mirrors `TypeScriptGeneratorConfig::tanstack`,
    /// and is read by the three `*-index.ts.j2` templates (gates the
    /// `./react-query.js` re-export) — `package.json.j2` reads
    /// `peer_dependencies`/`dev_dependencies` instead (see those fields'
    /// doc comments for why). `src/react-query.ts` itself is gated by spec
    /// selection (`crate::templates::specs`), not by this field.
    tanstack: bool,
    /// `package.json.j2`'s `peerDependencies` entries, joined by a
    /// `{% for %}` loop in the template rather than nested `{% if %}`
    /// blocks — see `crate::package_deps`'s module doc for why issue #617
    /// forced that change. Empty when `refine`/`swr`/`tanstack` are all
    /// off, which renders a valid empty `"peerDependencies": {}`.
    peer_dependencies: Vec<DependencyEntry>,
    /// Same shape and rationale as `peer_dependencies`, for
    /// `devDependencies` — see `crate::package_deps::dev_dependencies_for`.
    dev_dependencies: Vec<DependencyEntry>,
    /// Issue #610: `README.md.j2`'s "Optimistic concurrency" section
    /// documents `getWithResponse`/`ifMatch`, which only exist on REST
    /// output (RPC has no per-route `If-Match`/`ETag` concept — see
    /// `rest-client.ts.j2`'s doc comment on `getWithResponse` and
    /// `crate::templates::specs`'s module doc on why `rpc-client.ts.j2`
    /// was deliberately left untouched). `true` iff `schema.transport ==
    /// TransportStyle::Rest`.
    is_rest_transport: bool,
    /// Issue #610: whether any model in the schema declares `@version` —
    /// gates the same README section a second way, since the section's
    /// own prose is scoped to "a model with an `@version` field". A
    /// schema with no versioned model has no `If-Match`/`ETag`
    /// requirement to document at all.
    has_versioned_model: bool,
}

pub(crate) fn build_template_context(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<TemplateContext, TypeScriptGeneratorError> {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied_type_names = occupied_type_names(schema);
    let decimal_shapes = build_decimal_shapes(schema);
    let client_class_name = format!(
        "{}Client",
        to_pascal_case(&package_class_stem(&config.package_name))
    );

    let mut enums = schema.enums.iter().map(build_enum_view).collect::<Vec<_>>();
    let mut interfaces = Vec::new();
    for ty in &schema.types {
        interfaces.push(build_interface(
            &ty.name,
            &ty.fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
    }
    // `InterfaceKind::Model` forces every field optional to account for
    // partial `fields`/`include` projection on the wire. `--full-selection`
    // opts a generation run out of that: reuse `Plain`'s arity-driven
    // optionality (the schema's own nullable/required split) so consumers
    // who always fetch full objects get fully-required interfaces instead
    // of hand-rolling a narrowing type on top of the generator's output.
    let model_interface_kind = if config.full_selection {
        InterfaceKind::Plain
    } else {
        InterfaceKind::Model
    };
    for model in &schema.models {
        let scalar_fields = scalar_model_fields(model, &model_names);
        interfaces.push(build_interface(
            &model.name,
            &visible_model_fields(model),
            model_interface_kind,
            &enum_names,
        ));
        // cratestack#743: `Create<M>Input`/`Update<M>Input` are only
        // ever referenced from this model's own generated `create`/
        // `update` client methods (`rest-client.ts.j2`/
        // `rpc-client.ts.j2`), which are correspondingly omitted once
        // `ModelApiView::allows_create`/`allows_update` is `false` — so
        // emitting the interface anyway would be exactly the
        // "unreferenced Create<M>Input" the acceptance criteria forbid.
        // `allows_create` already folds in `model_allows_create`, so
        // this preserves that pre-existing gate unchanged and only adds
        // the new suppression check on top (see `ModelApiView`'s doc).
        let internal = cratestack_core::model_internal_actions(model);
        if model_allows_create(model) && !internal.contains("create") {
            interfaces.push(build_interface(
                &format!("Create{}Input", model.name),
                &scalar_fields
                    .iter()
                    .copied()
                    // `@computed` fields are resolver-backed and
                    // response-time only — never part of a create input,
                    // since the server struct never carries them either
                    // (`docs/design/computed-fields.md`).
                    .filter(|field| !is_computed_field(field))
                    .filter(|field| !is_generated_on_create(field))
                    .collect::<Vec<_>>(),
                InterfaceKind::Plain,
                &enum_names,
            ));
        }
        if !internal.contains("update") {
            interfaces.push(build_interface(
                &format!("Update{}Input", model.name),
                &scalar_fields
                    .iter()
                    .copied()
                    .filter(|field| !is_primary_key(field))
                    // `@computed` fields are never part of an update input
                    // either — same reasoning as the create input above.
                    .filter(|field| !is_computed_field(field))
                    .collect::<Vec<_>>(),
                InterfaceKind::Patch,
                &enum_names,
            ));
        }

        let where_interface = build_where_interface(model, &model_names);
        if let Some(where_interface) = where_interface.clone() {
            interfaces.push(where_interface);
        }
        enums.push(build_sort_field_view(model, &model_names));
        interfaces.push(build_order_by_clause_interface(model));
        interfaces.push(build_find_many_interface(model, where_interface.is_some()));
        // `docs/design/computed-fields.md`'s typed `computedParams` surface
        // (cratestack#stage4): only emitted for a model with at least one
        // *parameterized* computed field — see
        // `crate::views::build_computed_params_interface`'s doc comment.
        if let Some(computed_params_interface) = build_computed_params_interface(model) {
            interfaces.push(computed_params_interface);
        }
    }
    for procedure in &schema.procedures {
        let fields = procedure
            .args
            .iter()
            .map(|arg| Field {
                docs: arg.docs.clone(),
                name: arg.name.clone(),
                name_span: arg.name_span,
                ty: arg.ty.clone(),
                attributes: Vec::new(),
                span: arg.span,
            })
            .collect::<Vec<_>>();
        interfaces.push(build_interface(
            &crate::naming::procedure_wrapper_name(procedure, &occupied_type_names),
            &fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
    }

    let mut models = schema
        .models
        .iter()
        .map(build_model_api)
        .collect::<Vec<_>>();
    disambiguate_model_api_keys(&mut models);
    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect::<Vec<_>>();
    let query_procedures = procedures
        .iter()
        .filter(|procedure| procedure.kind == "query")
        .cloned()
        .collect();
    let mutation_procedures = procedures
        .iter()
        .filter(|procedure| procedure.kind == "mutation")
        .cloned()
        .collect();

    let refine_version_requirement = if config.refine {
        format!("^{}", env!("CARGO_PKG_VERSION"))
    } else {
        String::new()
    };

    Ok(TemplateContext {
        package_name: config.package_name.clone(),
        client_class_name,
        base_path: config.base_path.clone(),
        schema_sha256: config.schema_sha256.clone(),
        enums,
        interfaces,
        models,
        procedures,
        query_procedures,
        mutation_procedures,
        decimal_shapes,
        refine: config.refine,
        refine_version_requirement: refine_version_requirement.clone(),
        // Built only when the flag is on: a default run has no template
        // that reads this, and walking every model to fill a list nothing
        // renders would be wasted work on the hot path.
        refine_resources: if config.refine {
            build_refine_resources(schema)
        } else {
            Vec::new()
        },
        refine_resource_map_type: if config.refine {
            refine_resource_map_type(schema.transport).to_owned()
        } else {
            String::new()
        },
        swr: config.swr,
        tanstack: config.tanstack,
        peer_dependencies: peer_dependencies_for(config, &refine_version_requirement),
        dev_dependencies: dev_dependencies_for(config, &refine_version_requirement),
        is_rest_transport: schema.transport == cratestack_core::TransportStyle::Rest,
        has_versioned_model: schema
            .models
            .iter()
            .any(|model| version_field(model).is_some()),
    })
}
