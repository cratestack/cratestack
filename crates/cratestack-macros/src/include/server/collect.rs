//! Per-element token collection for the server composer. Walks the
//! parsed schema once and materializes every generator output the
//! final `quote!` needs into named fields on [`ServerCollected`].
//! Returning a struct (rather than inlining ~30 `.iter().map(...)`
//! chains into the orchestrator's `quote!{...}`) keeps the
//! orchestrator under the 200-LoC budget without changing emission
//! order.

mod models;
mod procedures;

use cratestack_core::Schema;
use proc_macro::TokenStream;
use syn::LitStr;

use crate::client::generate_client_module;
use crate::event::generate_event_module;
use crate::shared::schema_lit;
use crate::transport::{
    generate_model_op_descriptors, generate_model_rpc_dispatch_arms,
    generate_procedure_op_descriptor, generate_procedure_rpc_dispatch_arm,
};
use crate::types::{
    generate_custom_field_descriptors, generate_custom_field_resolver_methods, generate_enum_type,
    generate_type_struct,
};

pub(super) type Ts = proc_macro2::TokenStream;

pub(super) struct ServerCollected {
    pub(super) transport_style_str: String,
    pub(super) is_rpc: bool,
    pub(super) mixin_names: Vec<syn::LitStr>,
    pub(super) model_names: Vec<syn::LitStr>,
    pub(super) type_names: Vec<syn::LitStr>,
    pub(super) enum_names: Vec<syn::LitStr>,
    pub(super) procedure_names: Vec<syn::LitStr>,
    pub(super) type_structs: Vec<Ts>,
    pub(super) enum_types: Vec<Ts>,
    pub(super) custom_field_descriptors: Vec<Ts>,
    pub(super) custom_field_resolver_methods: Vec<Ts>,
    pub(super) model_structs: Vec<Ts>,
    pub(super) pg_from_row_impls: Vec<Ts>,
    pub(super) primary_key_accessor_impls: Vec<Ts>,
    pub(super) model_descriptors: Vec<Ts>,
    pub(super) field_modules: Vec<Ts>,
    pub(super) create_input_structs: Vec<Ts>,
    pub(super) update_input_structs: Vec<Ts>,
    pub(super) upsert_input_impls: Vec<Ts>,
    pub(super) model_accessors: Vec<Ts>,
    pub(super) bound_model_accessors: Vec<Ts>,
    pub(super) procedure_modules: Vec<Ts>,
    pub(super) procedure_registry_methods: Vec<Ts>,
    pub(super) procedure_axum_handler_defs: Vec<Ts>,
    pub(super) procedure_axum_routes: Vec<Ts>,
    pub(super) procedure_transport_constants: Vec<Ts>,
    pub(super) model_axum_handler_defs: Vec<Ts>,
    pub(super) model_axum_routes: Vec<Ts>,
    pub(super) model_transport_constants: Vec<Ts>,
    pub(super) op_descriptor_entries: Vec<Ts>,
    pub(super) route_transport_entries: Vec<Ts>,
    pub(super) rpc_dispatch_arms: Vec<Ts>,
    pub(super) generated_client_module: Ts,
    pub(super) generated_event_module: Ts,
}

pub(super) fn collect_server_schema(
    schema: &Schema,
    schema_path: &LitStr,
) -> Result<ServerCollected, TokenStream> {
    let model_name_set = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect();
    let enum_name_set = crate::shared::enum_name_set(&schema.enums);
    let auth = schema.auth.as_ref();
    let auth_required_default = schema.auth.is_some();
    let is_rpc = matches!(schema.transport, cratestack_core::TransportStyle::Rpc);

    let mixin_names = schema
        .mixins
        .iter()
        .map(|m| schema_lit(&m.name))
        .collect();
    let model_names = schema
        .models
        .iter()
        .map(|m| schema_lit(&m.name))
        .collect();
    let type_names = schema.types.iter().map(|t| schema_lit(&t.name)).collect();
    let enum_names = schema.enums.iter().map(|e| schema_lit(&e.name)).collect();
    let procedure_names = schema
        .procedures
        .iter()
        .map(|p| schema_lit(&p.name))
        .collect();
    let type_structs = schema
        .types
        .iter()
        .map(|ty| generate_type_struct(ty, &enum_name_set))
        .collect();
    let enum_types = schema.enums.iter().map(generate_enum_type).collect();
    let custom_field_descriptors = schema
        .types
        .iter()
        .flat_map(|ty| generate_custom_field_descriptors(ty).into_iter())
        .collect();
    let custom_field_resolver_methods = schema
        .types
        .iter()
        .flat_map(|ty| generate_custom_field_resolver_methods(ty).into_iter())
        .collect();

    let mc = models::collect_models(schema, schema_path, &model_name_set, &enum_name_set, auth)?;
    let pc = procedures::collect_procedures(schema, schema_path, &enum_name_set, auth)?;

    // RPC op descriptors + dispatch arms — see docs/design/rpc-transport.md.
    // Both `OPS` and `ROUTE_TRANSPORTS` consts are always emitted (for uniform
    // introspection), but the schema's `transport` directive picks which slice
    // is non-empty.
    let (op_descriptor_entries, route_transport_entries) = if is_rpc {
        let mut ops = Vec::new();
        for procedure in &schema.procedures {
            ops.push(generate_procedure_op_descriptor(procedure, auth_required_default));
        }
        for model in &schema.models {
            ops.extend(generate_model_op_descriptors(model, auth_required_default));
        }
        (ops, Vec::new())
    } else {
        let mut routes = Vec::new();
        routes.extend(pc.transport_entries.iter().cloned());
        routes.extend(mc.transport_entries.iter().cloned());
        (Vec::new(), routes)
    };

    let rpc_dispatch_arms: Vec<Ts> = if is_rpc {
        let mut arms = Vec::new();
        for procedure in &schema.procedures {
            arms.push(generate_procedure_rpc_dispatch_arm(procedure));
        }
        for model in &schema.models {
            arms.extend(generate_model_rpc_dispatch_arms(model));
        }
        arms
    } else {
        Vec::new()
    };

    let generated_client_module =
        generate_client_module(&schema.models, &schema.procedures, schema.transport)
            .map_err(|e| compile_error(schema_path, e))?;
    let generated_event_module =
        generate_event_module(&schema.models).map_err(|e| compile_error(schema_path, e))?;

    Ok(ServerCollected {
        transport_style_str: schema.transport.as_str().to_owned(),
        is_rpc,
        mixin_names,
        model_names,
        type_names,
        enum_names,
        procedure_names,
        type_structs,
        enum_types,
        custom_field_descriptors,
        custom_field_resolver_methods,
        model_structs: mc.structs,
        pg_from_row_impls: mc.pg_from_row_impls,
        primary_key_accessor_impls: mc.primary_key_accessor_impls,
        model_descriptors: mc.descriptors,
        field_modules: mc.field_modules,
        create_input_structs: mc.create_input_structs,
        update_input_structs: mc.update_input_structs,
        upsert_input_impls: mc.upsert_input_impls,
        model_accessors: mc.accessors,
        bound_model_accessors: mc.bound_accessors,
        procedure_modules: pc.modules,
        procedure_registry_methods: pc.registry_methods,
        procedure_axum_handler_defs: pc.axum_handler_defs,
        procedure_axum_routes: pc.axum_routes,
        procedure_transport_constants: pc.transport_constants,
        model_axum_handler_defs: mc.axum_handler_defs,
        model_axum_routes: mc.axum_routes,
        model_transport_constants: mc.transport_constants,
        op_descriptor_entries,
        route_transport_entries,
        rpc_dispatch_arms,
        generated_client_module,
        generated_event_module,
    })
}

pub(super) fn compile_error(schema_path: &LitStr, error: String) -> TokenStream {
    syn::Error::new(schema_path.span(), error)
        .to_compile_error()
        .into()
}
