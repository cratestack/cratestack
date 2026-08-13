use cratestack_core::{Field, Schema, TransportStyle};
use serde::Serialize;

use crate::config::TypeScriptGeneratorConfig;
use crate::decimal::{DecimalShapeView, build_decimal_shapes};
use crate::error::TypeScriptGeneratorError;
use crate::find_many_views::{
    build_find_many_interface, build_order_by_clause_interface, build_sort_field_view,
    build_where_interface,
};
use crate::grpc::GrpcContext;
use crate::naming::{occupied_type_names, package_class_stem, to_pascal_case};
use crate::procedure_views::{ProcedureView, build_procedure};
use crate::refine::{RefineResourceView, build_refine_resources};
use crate::types::{
    enum_name_set, is_generated_on_create, is_primary_key, model_allows_create, model_name_set,
    scalar_model_fields, visible_model_fields,
};
use crate::views::{
    EnumView, InterfaceKind, InterfaceView, ModelApiView, build_enum_view, build_interface,
    build_model_api, disambiguate_model_api_keys,
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
    /// Only set for `transport grpc` schemas — see `crate::grpc`'s module
    /// doc. `None` for REST/RPC, where the REST/RPC-specific templates
    /// never reference `grpc.*` in the first place.
    grpc: Option<GrpcContext>,
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
        if model_allows_create(model) {
            interfaces.push(build_interface(
                &format!("Create{}Input", model.name),
                &scalar_fields
                    .iter()
                    .copied()
                    .filter(|field| !is_generated_on_create(field))
                    .collect::<Vec<_>>(),
                InterfaceKind::Plain,
                &enum_names,
            ));
        }
        interfaces.push(build_interface(
            &format!("Update{}Input", model.name),
            &scalar_fields
                .iter()
                .copied()
                .filter(|field| !is_primary_key(field))
                .collect::<Vec<_>>(),
            InterfaceKind::Patch,
            &enum_names,
        ));

        let where_interface = build_where_interface(model, &model_names);
        if let Some(where_interface) = where_interface.clone() {
            interfaces.push(where_interface);
        }
        enums.push(build_sort_field_view(model, &model_names));
        interfaces.push(build_order_by_clause_interface(model));
        interfaces.push(build_find_many_interface(model, where_interface.is_some()));
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
    // `transport grpc` never routes procedures at all — ticket #171 didn't
    // wire them into the generated tonic service (see `crate::grpc`'s
    // module doc) — so a gRPC-Web client exposing `.procedures.foo()`
    // would only ever hit `Unimplemented`. Empty rather than generated but
    // dead.
    let (procedures, query_procedures, mutation_procedures) =
        if schema.transport == TransportStyle::Grpc {
            (Vec::new(), Vec::new(), Vec::new())
        } else {
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
            (procedures, query_procedures, mutation_procedures)
        };

    let grpc = crate::grpc::build_grpc_context(schema, config.pb_lock.as_ref())?;

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
        grpc,
        refine: config.refine,
        refine_version_requirement: if config.refine {
            format!("^{}", env!("CARGO_PKG_VERSION"))
        } else {
            String::new()
        },
        // Built only when the flag is on: a default run has no template
        // that reads this, and walking every model to fill a list nothing
        // renders would be wasted work on the hot path.
        refine_resources: if config.refine {
            build_refine_resources(schema)
        } else {
            Vec::new()
        },
    })
}
