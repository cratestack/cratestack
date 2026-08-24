//! Builds the five WireMock stub-mapping JSON documents
//! (`list`/`get`/`create`/`update`/`delete`) a `model` block's CRUD
//! routes need. `transport rest` schemas get the stateful generator
//! (`crate::model_state` — real per-record create/list/update/delete
//! against `wiremock-state-extension`, see `docs/design/
//! wiremock-stubs.md`'s "Model CRUD statefulness" section);
//! `transport rpc` schemas still get the static v1 baseline (one
//! deterministic example record, no state), because the extension's
//! per-record context needs a value unique to *this* request that isn't
//! the id-bearing URL path RPC doesn't have — see [`rpc`]'s module doc
//! for the full reasoning.

mod rpc;

use std::collections::BTreeSet;

use cratestack_core::{Model, Schema, TransportStyle};
use serde_json::{Value, json};

use crate::config::WireMockGeneratorConfig;
use crate::error::WireMockGeneratorError;
use crate::model_attrs::{is_paged_model, is_primary_key};
use crate::model_record::{list_envelope, synthesize_model_record};
use crate::model_state::build_stateful_rest_mappings;

/// One verb's request-match shape — only used by the static (`rpc`)
/// path now; the stateful REST path builds its own richer mapping
/// shape directly (`crate::model_state`).
pub(crate) struct VerbRoute {
    pub(crate) verb: &'static str,
    pub(crate) method: &'static str,
    pub(crate) url: String,
    pub(crate) status: u16,
}

/// Builds the `(verb, mapping)` pairs for every CRUD route `model`
/// declares. Static `["list", "get", "create", "update", "delete"]`
/// order for `transport rpc`; for `transport rest` see
/// `build_stateful_rest_mappings`'s own doc for how an `@version` model
/// fans `update`/`delete` out into more than one pair each.
pub(crate) fn build_model_mappings(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<Vec<(String, Value)>, WireMockGeneratorError> {
    match schema.transport {
        TransportStyle::Rest => build_stateful_rest_mappings(schema, config, model, model_names),
        TransportStyle::Rpc => build_static_rpc_mappings(schema, config, model, model_names),
    }
}

/// The pre-stateful v1 shape, kept for `transport rpc` (see the module
/// doc).
fn build_static_rpc_mappings(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<Vec<(String, Value)>, WireMockGeneratorError> {
    if !model.fields.iter().any(is_primary_key) {
        return Err(WireMockGeneratorError::ModelMissingPrimaryKey {
            model: model.name.clone(),
        });
    }

    let record = synthesize_model_record(schema, model, model_names)?;
    let list_body = list_envelope(is_paged_model(model), &record);
    let base = config.base_path.trim_end_matches('/');
    let routes = rpc::rpc_routes(base, &model.name);
    // Same order as `routes`: list, get, create, update, delete.
    let bodies = [&list_body, &record, &record, &record, &record];

    Ok(routes
        .into_iter()
        .zip(bodies)
        .map(|(route, body)| {
            (
                route.verb.to_owned(),
                build_static_mapping(&route, body, &model.name),
            )
        })
        .collect())
}

fn build_static_mapping(route: &VerbRoute, body: &Value, model_name: &str) -> Value {
    json!({
        // `urlPath` matches on the request path only — WireMock ignores
        // any query string entirely unless a `queryParameters` matcher is
        // also present, and none is added here. This is load-bearing for
        // `?computedParams=` (`docs/design/computed-fields.md`): a real
        // client's `get`/`list` call carrying a `computedParams` query
        // parameter still matches this stub exactly like a request with
        // none, since the generator has no way to synthesize a response
        // that varies per resolver-params value anyway (see
        // `docs/design/wiremock-stubs.md`'s "How do callers vary
        // responses?" open question). Do not add a `queryParameters`
        // matcher here without also handling `computedParams`.
        "request": { "method": route.method, "urlPath": route.url },
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
