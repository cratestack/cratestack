//! Per-procedure token gathering for [`super::collect_server_schema`]:
//! procedure module, registry method, axum handler/route, transport
//! constants.

use std::collections::BTreeSet;

use cratestack_core::Schema;
use proc_macro::TokenStream;
use syn::LitStr;

use crate::axum::{generate_procedure_axum_handler, generate_procedure_axum_route};
use crate::procedure::{generate_procedure_module, generate_procedure_registry_method};
use crate::transport::{
    generate_procedure_transport_constants, generate_procedure_transport_entries,
};

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
) -> Result<ProcedureCollected, TokenStream> {
    let modules = schema
        .procedures
        .iter()
        .map(|procedure| {
            generate_procedure_module(procedure, &schema.models, &schema.types, enum_name_set, auth)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let registry_methods = schema
        .procedures
        .iter()
        .map(generate_procedure_registry_method)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let axum_handler_defs = schema
        .procedures
        .iter()
        .map(generate_procedure_axum_handler)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| compile_error(schema_path, e))?;
    let axum_routes = schema
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
    let transport_entries = schema
        .procedures
        .iter()
        .map(generate_procedure_transport_entries)
        .collect();

    Ok(ProcedureCollected {
        modules,
        registry_methods,
        axum_handler_defs,
        axum_routes,
        transport_constants,
        transport_entries,
    })
}
