//! Per-procedure token gathering for [`super::collect_server_schema`]:
//! procedure module, registry method, axum handler/route, transport
//! constants.

use std::collections::BTreeSet;

use cratestack_core::Schema;
use proc_macro::TokenStream;
use syn::LitStr;

use crate::axum::{
    generate_procedure_axum_handler, generate_procedure_axum_route,
    generate_subscription_procedure_axum,
};
use crate::procedure::{
    generate_procedure_module, generate_procedure_registry_method,
    generate_subscription_registry_method,
};
use crate::transport::{
    generate_procedure_transport_constants, generate_procedure_transport_entries,
    generate_subscription_procedure_route_transport,
};
use crate::validators::Validating;

use super::{Ts, compile_error};

pub(super) struct ProcedureCollected {
    pub(super) modules: Vec<Ts>,
    pub(super) registry_methods: Vec<Ts>,
    pub(super) axum_handler_defs: Vec<Ts>,
    pub(super) axum_routes: Vec<Ts>,
    pub(super) transport_constants: Vec<Ts>,
    pub(super) transport_entries: Vec<Ts>,
}

pub(super) fn collect_procedures(
    schema: &Schema,
    schema_path: &LitStr,
    enum_name_set: &BTreeSet<&str>,
    auth: Option<&cratestack_core::AuthBlock>,
    bearing: &BTreeSet<String>,
    validating: &Validating,
) -> Result<ProcedureCollected, TokenStream> {
    // `subscription procedure`s (slice S4) share a procedure's module (its
    // `Args`, policies and `authorize_with_db`) and registry, not its routes.
    let modules = schema
        .procedures
        .iter()
        .chain(&schema.subscription_procedures)
        .map(|procedure| {
            generate_procedure_module(
                procedure,
                &schema.models,
                &schema.types,
                enum_name_set,
                auth,
                validating,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let mut registry_methods = schema
        .procedures
        .iter()
        .map(generate_procedure_registry_method)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    registry_methods.extend(
        schema
            .subscription_procedures
            .iter()
            .map(generate_subscription_registry_method),
    );
    let axum_handler_defs = schema
        .procedures
        .iter()
        .map(|procedure| generate_procedure_axum_handler(procedure, bearing))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let axum_routes: Vec<Ts> = schema
        .procedures
        .iter()
        .map(generate_procedure_axum_route)
        .collect();
    let transport_constants = schema
        .procedures
        .iter()
        .map(generate_procedure_transport_constants)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let transport_entries: Vec<Ts> = schema
        .procedures
        .iter()
        .map(generate_procedure_transport_entries)
        .collect();

    let (mut axum_handler_defs, mut axum_routes, mut transport_constants, mut transport_entries) = (
        axum_handler_defs,
        axum_routes,
        transport_constants,
        transport_entries,
    );
    for procedure in &schema.subscription_procedures {
        let (handler, route) = generate_subscription_procedure_axum(procedure);
        axum_handler_defs.push(handler);
        axum_routes.push(route);
        let (constant, entry) = generate_subscription_procedure_route_transport(procedure);
        transport_constants.push(constant);
        transport_entries.push(entry);
    }

    Ok(ProcedureCollected {
        modules,
        registry_methods,
        axum_handler_defs,
        axum_routes,
        transport_constants,
        transport_entries,
    })
}
