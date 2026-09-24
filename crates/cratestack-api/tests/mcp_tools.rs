//! cratestack#1038 (MCP phase 3): the generated `cratestack_schema::mcp`
//! module served over stdio framing — listing, policy, argument errors and
//! `@computed` outputs — for a `db = None` schema. Admission (idempotency,
//! rate limiting) is `tests/mcp_admission.rs`; row policy through the ORM
//! is `cratestack-pg`'s `tests/mcp_policy_pg.rs`.
//!
//! Gated `required-features = ["mcp"]`; `just test-ci-host` runs it.

mod mcp_support;

use cratestack::mcp::StdioServer;
use cratestack::{CratestackContext, SystemContext};
use mcp_support::client::{Client, envelope};
use mcp_support::{Registry, caller, tools};
use serde_json::{Value, json};

fn serve(registry: &Registry, ctx: CratestackContext) -> Client {
    Client::start(StdioServer::new(tools(registry), ctx).expect("generated table is valid"))
}

#[tokio::test]
async fn tools_list_is_exactly_the_annotated_tools_in_declaration_order() {
    let mut client = serve(&Registry::default(), caller("u-1", "teller"));

    let discovered = client.request("server/discover", json!({})).await;
    assert_eq!(
        discovered["result"]["supportedVersions"],
        json!(["2026-07-28"])
    );

    let listed = client.request("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools").clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["whoami", "transfer_funds", "touch", "badge"],
        "declaration order; `internalOnly` has no `@mcp(tool)` and must not appear"
    );

    let hints: Vec<&Value> = tools.iter().map(|t| &t["annotations"]).collect();
    assert_eq!(*hints[0], json!({ "readOnlyHint": true }));
    assert_eq!(
        *hints[1],
        json!({ "readOnlyHint": false, "idempotentHint": false }),
        "a mutation that takes reservations"
    );
    // cratestack#1038 decision 3: a `@no_idempotency` mutation opts out of
    // reservations, so it must not claim to be safe to retry — even though
    // its generated descriptor is `idempotent_by_default`, which is what the
    // hint used to be read from. The first assertion keeps this test from
    // passing vacuously if the fixture ever stops exercising that case.
    let touch = &mcp_support::cratestack_schema::mcp::TOOLS[2];
    assert_eq!(touch.name, "touch");
    assert!(
        touch.op.idempotent_by_default,
        "`@no_idempotency` is `idempotent_by_default` in the generated TOOLS"
    );
    assert_eq!(
        *hints[2],
        json!({ "readOnlyHint": false, "idempotentHint": false }),
        "`@no_idempotency` must never advertise `idempotentHint: true`"
    );
    assert_eq!(tools[1]["description"], "Move money between accounts.");

    // Byte-for-byte the phase 2 schemas the generated table embeds.
    for (listed, generated) in tools
        .iter()
        .zip(mcp_support::cratestack_schema::mcp::TOOLS.iter())
    {
        let input: Value = serde_json::from_str(generated.input_schema).unwrap();
        assert_eq!(listed["inputSchema"], input, "{}", generated.name);
        let output = generated
            .output_schema
            .map(|s| serde_json::from_str::<Value>(s).unwrap());
        assert_eq!(
            listed.get("outputSchema").cloned(),
            output,
            "{}",
            generated.name
        );
    }
}

/// The decisive policy test (cratestack#1038). `transfer_funds` is
/// `@allow(auth().role == "teller")`; a clerk's call must be refused as an
/// `isError` result, and the implementation must never run. Loosening the
/// fixture's `@allow` to `@allow(true)` makes this fail.
#[tokio::test]
async fn a_call_its_allow_refuses_is_an_error_and_never_runs() {
    let registry = Registry::default();
    let mut clerk = serve(&registry, caller("u-2", "clerk"));
    let denied = clerk
        .call("transfer_funds", json!({ "args": { "amount": 10 } }), None)
        .await;
    let error = envelope(&denied);
    assert_eq!(error["code"], "FORBIDDEN", "{error}");
    assert_eq!(
        registry.runs(),
        0,
        "the implementation must not run for a denied call"
    );

    // Positive control: the same call as a teller runs, so the counter is live.
    let mut teller = serve(&registry, caller("u-1", "teller"));
    let allowed = teller
        .call("transfer_funds", json!({ "args": { "amount": 10 } }), None)
        .await;
    assert_eq!(
        allowed["structuredContent"],
        json!({ "amount": 10, "run": 1 })
    );
    assert_eq!(registry.runs(), 1);
}

#[tokio::test]
async fn an_unknown_or_unexposed_tool_is_invalid_params() {
    let registry = Registry::default();
    let mut client = serve(&registry, caller("u-1", "teller"));
    for name in ["nope", "internalOnly", "transfer"] {
        let response = client
            .request("tools/call", json!({ "name": name, "arguments": {} }))
            .await;
        assert_eq!(
            response["error"]["code"],
            json!(-32602),
            "{name}: {response}"
        );
    }
    assert_eq!(registry.runs(), 0);
}

#[tokio::test]
async fn arguments_that_do_not_decode_name_the_field() {
    let registry = Registry::default();
    let mut client = serve(&registry, caller("u-1", "teller"));
    let result = client
        .call(
            "transfer_funds",
            json!({ "args": { "amount": "ten" } }),
            None,
        )
        .await;
    let error = envelope(&result);
    assert_eq!(error["code"], "VALIDATION_ERROR");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("`args.amount`"), "{message}");
    assert_eq!(registry.runs(), 0);
}

/// ADR 0002 Q7: a `@computed` output is resolved by the same composition
/// REST runs, and the result validates against the advertised
/// `outputSchema` — the round trip phase 2 could not do for these types.
#[tokio::test]
async fn a_computed_output_is_composed_and_matches_its_output_schema() {
    let mut client = serve(&Registry::default(), caller("u-1", "clerk"));
    let result = client.call("badge", json!({ "label": "hi" }), None).await;
    let value = &result["structuredContent"];
    assert_eq!(
        *value,
        json!({ "label": "hi", "shout": "HI", "note": "for u-1" })
    );

    let listed = client.request("tools/list", json!({})).await;
    let schema = listed["result"]["tools"][3]["outputSchema"].clone();
    assert!(
        schema["$defs"]["Badge"]["properties"]["shout"].is_object(),
        "{schema}"
    );
    let validator = jsonschema::draft202012::new(&schema).expect("output schema compiles");
    assert!(
        validator.is_valid(value),
        "composed output fails its own schema"
    );

    // `note` is `String?`: `null` when the resolver has nothing, and the
    // schema still accepts it.
    let mut system = serve(
        &Registry::default(),
        SystemContext::for_service("s").into_context(),
    );
    let anonymous = system.call("badge", json!({ "label": "x" }), None).await;
    assert_eq!(
        anonymous["structuredContent"]["note"],
        json!("for system:s")
    );
    let mut nulled = anonymous["structuredContent"].clone();
    nulled["note"] = Value::Null;
    assert!(validator.is_valid(&nulled));
}
