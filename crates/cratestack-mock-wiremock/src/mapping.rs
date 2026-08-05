//! Builds one WireMock stub-mapping JSON document
//! (<https://wiremock.org/docs/stubbing/>) per procedure.

use cratestack_core::{Procedure, ProcedureKind, Schema, TransportStyle};
use serde_json::{Value, json};

use crate::config::WireMockGeneratorConfig;
use crate::error::WireMockGeneratorError;
use crate::values::synthesize;

/// Every procedure — REST or RPC transport alike — is `POST`, always
/// answers `200` on success (`crates/cratestack-macros/src/axum/
/// procedure.rs` hardcodes `StatusCode::OK`; there is no per-procedure
/// override yet, tracked upstream as cratestack#407), and carries its
/// return type as the whole JSON body with no envelope. That's exactly
/// what v1 stubs: request-match on method + path only (no body
/// assertion — see the design doc's "How do callers vary responses?"
/// open question for why), respond `200` with a synthesized instance of
/// the declared return type.
pub(crate) fn build_procedure_mapping(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
    procedure: &Procedure,
) -> Result<Value, WireMockGeneratorError> {
    let mut in_progress = Vec::new();
    let body = synthesize(
        schema,
        &procedure.name,
        &procedure.return_type,
        &mut in_progress,
    )?;

    let route_path = match schema.transport {
        // `RPC_UNARY_PATH` (`cratestack_core::rpc`) is `/rpc/{op_id}`;
        // `{op_id}` is the procedure name.
        TransportStyle::Rpc => format!("/rpc/{}", procedure.name),
        // REST is the schema default and the only other transport this
        // generator supports (`generate_package` rejects `Grpc` before
        // this is reached) — every procedure's REST route is
        // `/$procs/{name}` regardless of `transport rest` being
        // implicit or explicit (`generate_procedure_transport_constants`
        // in `crates/cratestack-macros/src/transport/rest.rs`).
        TransportStyle::Rest | TransportStyle::Grpc => format!("/$procs/{}", procedure.name),
    };
    let url_path = format!("{}{route_path}", config.base_path.trim_end_matches('/'));

    Ok(json!({
        "request": {
            "method": "POST",
            "urlPath": url_path,
        },
        "response": {
            "status": 200,
            "headers": { "Content-Type": "application/json" },
            "jsonBody": body,
        },
        "metadata": {
            "cratestack": {
                "generated": true,
                "procedure": procedure.name,
                "kind": match procedure.kind {
                    ProcedureKind::Query => "query",
                    ProcedureKind::Mutation => "mutation",
                },
            },
        },
    }))
}
