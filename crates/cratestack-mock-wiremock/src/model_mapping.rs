//! Builds the five WireMock stub-mapping JSON documents
//! (`list`/`get`/`create`/`update`/`delete`) a `model` block's REST or
//! RPC CRUD routes need. Route derivation delegates to [`rest`]/[`rpc`]
//! (transport-specific path shapes); the response body — one
//! deterministic example record, or that record wrapped in the `list`
//! envelope — is shared across every verb and both transports, since
//! REST and RPC dispatch to the exact same handler body for a given
//! verb (`crates/cratestack-macros/src/axum/model/handlers_crud.rs`'s
//! doc comments: "RPC dispatch passes `POST /rpc/model.<M>.<verb>` with
//! ..." on each REST handler).

mod rest;
mod rpc;

use std::collections::BTreeSet;

use cratestack_core::{Model, Schema, TransportStyle};
use serde_json::{Value, json};

use crate::config::WireMockGeneratorConfig;
use crate::error::WireMockGeneratorError;
use crate::model_attrs::{is_paged_model, is_primary_key};
use crate::model_record::{list_envelope, synthesize_model_record};

/// One verb's request-match shape, transport-agnostic.
pub(crate) struct VerbRoute {
    pub(crate) verb: &'static str,
    pub(crate) method: &'static str,
    /// An exact `urlPath` if `is_pattern` is false, a `urlPathPattern`
    /// regex otherwise (REST's `get`/`update`/`delete` need a `{id}`
    /// wildcard; RPC's five routes are always exact — see `rpc.rs`).
    pub(crate) url: String,
    pub(crate) is_pattern: bool,
    pub(crate) status: u16,
}

/// Builds the `(verb, mapping)` pairs for every CRUD route `model`
/// declares, in `["list", "get", "create", "update", "delete"]` order —
/// matching `generate_model_axum_routes`
/// (`crates/cratestack-macros/src/axum/model/routes.rs`) for REST, and
/// `generate_model_rpc_dispatch_arms`
/// (`crates/cratestack-macros/src/transport/rpc.rs`) for RPC.
pub(crate) fn build_model_mappings(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<Vec<(&'static str, Value)>, WireMockGeneratorError> {
    if !model.fields.iter().any(is_primary_key) {
        return Err(WireMockGeneratorError::ModelMissingPrimaryKey {
            model: model.name.clone(),
        });
    }

    let record = synthesize_model_record(schema, model, model_names)?;
    let list_body = list_envelope(is_paged_model(model), &record);
    let base = config.base_path.trim_end_matches('/');

    let routes = match schema.transport {
        TransportStyle::Rpc => rpc::rpc_routes(base, &model.name),
        TransportStyle::Rest | TransportStyle::Grpc => {
            let plural = cratestack_core::route_naming::model_route_segment(&model.name);
            rest::rest_routes(base, &plural)
        }
    };
    // Same order as `routes`: list, get, create, update, delete.
    let bodies = [&list_body, &record, &record, &record, &record];

    Ok(routes
        .into_iter()
        .zip(bodies)
        .map(|(route, body)| (route.verb, build_mapping(&route, body, &model.name)))
        .collect())
}

fn build_mapping(route: &VerbRoute, body: &Value, model_name: &str) -> Value {
    let request = if route.is_pattern {
        json!({ "method": route.method, "urlPathPattern": route.url })
    } else {
        json!({ "method": route.method, "urlPath": route.url })
    };

    json!({
        "request": request,
        "response": {
            "status": route.status,
            "headers": { "Content-Type": "application/json" },
            "jsonBody": body,
        },
        "metadata": {
            "cratestack": {
                "generated": true,
                "model": model_name,
                "operation": route.verb,
            },
        },
    })
}
