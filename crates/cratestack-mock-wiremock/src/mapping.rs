//! Builds one WireMock stub-mapping JSON document
//! (<https://wiremock.org/docs/stubbing/>) per procedure.

use cratestack_core::{Procedure, ProcedureKind, Schema, TransportStyle};
use serde_json::{Value, json};

use crate::config::WireMockGeneratorConfig;
use crate::error::WireMockGeneratorError;
use crate::values::synthesize;

/// Every procedure — REST or RPC transport alike — is `POST` and carries
/// its return type as the whole JSON body with no envelope. The success
/// status is the procedure's `@status(<code>)` (2xx only, REST-only: the
/// parser rejects it under `transport rpc`) and `200` otherwise, read the
/// same way `procedure_success_status_tokens` does in
/// `crates/cratestack-macros/src/axum/procedure/route_attrs.rs`. v1 stubs
/// request-match on method + path only (no body assertion — see the design
/// doc's "How do callers vary responses?" open question for why) and
/// respond with a synthesized instance of the declared return type.
pub(crate) fn build_procedure_mapping(
    schema: &Schema,
    config: &WireMockGeneratorConfig,
    procedure: &Procedure,
) -> Result<Value, WireMockGeneratorError> {
    let owner = format!("procedure `{}`", procedure.name);
    let mut in_progress = Vec::new();
    let body = synthesize(schema, &owner, &procedure.return_type, &mut in_progress)?;

    let route_path = match schema.transport {
        // `RPC_UNARY_PATH` (`cratestack_core::rpc`) is `/rpc/{op_id}`, but
        // `{op_id}` is NOT the bare procedure name — it's `procedure.<name>`
        // (`generate_procedure_rpc_dispatch_arm` in
        // `crates/cratestack-macros/src/transport/rpc.rs`:
        // `let op_id = format!("procedure.{}", procedure.name);`, matched
        // against the identical `'procedure.{{ procedure.name }}'` the
        // generated Dart RPC client sends in
        // `templates/rpc-apis.dart.j2`, and exercised end-to-end by
        // `crates/cratestack-pg/tests/rpc_canonical_request.rs` and
        // `crates/cratestack-pg/tests/include_schema.rs`, both of which
        // hit `/rpc/procedure.ping`, never `/rpc/ping`). Omitting the
        // `procedure.` prefix here would make every RPC-transport stub
        // silently never match a real client's request.
        TransportStyle::Rpc => format!(
            "/rpc/{}",
            cratestack_core::procedure_op_key(procedure, true)
        ),
        // REST is the schema default and the only other transport this
        // generator supports. The route is `/$procs/{name}`, or
        // `/{version}/$procs/{name}` under `@api_version`, whether
        // `transport rest` is implicit or explicit; derived by the same
        // function the server mounts with.
        TransportStyle::Rest => {
            cratestack_core::procedure_route::procedure_rest_route_path(procedure)
        }
    };
    let url_path = format!("{}{route_path}", config.base_path.trim_end_matches('/'));

    Ok(json!({
        "request": {
            "method": "POST",
            // Path-only match, same as `model_mapping::build_static_mapping`
            // — no `queryParameters` matcher, so any query string a caller
            // sends is ignored. Procedures have no `computedParams` surface
            // at all (`docs/design/computed-fields.md` scopes that to model
            // `get`/`list`), but this is the same load-bearing property:
            // don't add a `queryParameters` matcher here without checking
            // whether it would also need to account for that.
            "urlPath": url_path,
        },
        "response": {
            "status": success_status(procedure),
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

/// The procedure's declared `@status(<code>)`, defaulting to `200`.
fn success_status(procedure: &Procedure) -> u16 {
    procedure
        .attributes
        .iter()
        .find_map(|attribute| {
            attribute
                .raw
                .strip_prefix("@status(")
                .and_then(|rest| rest.strip_suffix(')'))
                .and_then(|inner| inner.trim().parse::<u16>().ok())
        })
        .unwrap_or(200)
}
