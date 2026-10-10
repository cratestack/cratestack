//! ADR 0019: an MCP `tools/call` with a `BigInt` argument goes through the
//! same canonical-string serde as REST and RPC. A canonical decimal string
//! decodes to the exact `i64` (the registry records what it received), a
//! JSON number is refused with an error that names the argument, and the
//! implementation never runs for a refused call.
//!
//! MCP is outside the transport-parity rule (`CLAUDE.md`), but the decode is
//! the generated `Args`' `Deserialize`, so there is no MCP-specific path to
//! forget. Gated on the `mcp` feature by the `cfg` below, because the
//! generated `cratestack_schema::mcp` module only exists with it;
//! `just test-ci-host` runs this crate with the feature on.

#![cfg(feature = "mcp")]

#[allow(dead_code)] // Each test binary uses a different subset of the client.
#[path = "mcp_support/client.rs"]
mod client;

use std::sync::{Arc, Mutex};

use client::{Client, envelope};
use cratestack::include_server_schema;
use cratestack::mcp::StdioServer;
use cratestack::{CratestackContext, CratestackError, SystemContext};
use serde_json::{Value, json};

include_server_schema!("tests/fixtures/mcp_tools_bigint.cstack", db = None);

use cratestack_schema::Ledger;
use cratestack_schema::procedures::{echo_big, echo_ledger};

/// ADR 0019's three pinned values, as the text the wire carries.
const PINNED: [(&str, i64); 3] = [
    ("9223372036854775807", i64::MAX),
    ("-9223372036854775808", i64::MIN),
    ("9007199254740993", 9_007_199_254_740_993),
];

/// Records the `total` of every call it runs: "refused" can be read off the
/// result, "never ran" only off a side effect the implementation would have
/// had, and "decoded exactly" only off what it received.
#[derive(Clone, Default)]
struct Registry {
    seen: Arc<Mutex<Vec<i64>>>,
}

impl Registry {
    fn seen(&self) -> Vec<i64> {
        self.seen.lock().unwrap().clone()
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for Registry {
    async fn echo_big(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: echo_big::Args,
        _authorized: echo_big::Authorized,
    ) -> Result<echo_big::Output, CratestackError> {
        self.seen.lock().unwrap().push(args.total.get());
        Ok(Ledger {
            total: args.total,
            note: args.maybe,
            history: args.totals,
        })
    }

    async fn echo_ledger(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: echo_ledger::Args,
        _authorized: echo_ledger::Authorized,
    ) -> Result<echo_ledger::Output, CratestackError> {
        self.seen.lock().unwrap().push(args.ledger.total.get());
        Ok(args.ledger)
    }
}

fn serve(registry: &Registry) -> Client {
    let tools = cratestack_schema::mcp::tools(
        cratestack_schema::Cratestack::builder().build(),
        registry.clone(),
        (),
    );
    let ctx = SystemContext::for_service("bigint").into_context();
    Client::start(StdioServer::new(tools, ctx).expect("generated table is valid"))
}

fn fragment() -> Value {
    json!({ "type": "string", "pattern": "^(0|-?[1-9][0-9]{0,18})$" })
}

async fn output_validator(client: &mut Client, tool: &str) -> jsonschema::Validator {
    let listed = client.request("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let listed = tools.iter().find(|t| t["name"] == tool).expect("listed");
    let schema = listed["outputSchema"].clone();
    jsonschema::draft202012::new(&schema).expect("output schema compiles")
}

#[tokio::test]
async fn tools_list_advertises_the_canonical_decimal_string() {
    let mut client = serve(&Registry::default());
    let listed = client.request("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let echo_big = &tools[0];
    assert_eq!(echo_big["name"], "echoBig");
    let input = &echo_big["inputSchema"];
    assert_eq!(input["properties"]["total"], fragment());
    assert_eq!(
        input["properties"]["maybe"],
        json!({ "anyOf": [fragment(), { "type": "null" }] })
    );
    assert_eq!(
        input["properties"]["totals"],
        json!({ "type": "array", "items": fragment() })
    );
    let ledger = &echo_big["outputSchema"]["$defs"]["Ledger"]["properties"];
    assert_eq!(ledger["total"], fragment());
    assert_eq!(ledger["history"]["items"], fragment());
    assert!(!input.to_string().contains("integer"), "{input}");
}

#[tokio::test]
async fn a_canonical_string_decodes_exactly_and_returns_as_a_string() {
    let registry = Registry::default();
    let mut client = serve(&registry);
    let validator = output_validator(&mut client, "echoBig").await;
    for (text, _) in PINNED {
        let arguments = json!({ "total": text, "maybe": text, "totals": [text, "0"] });
        let result = client.call("echoBig", arguments, None).await;
        assert_ne!(result["isError"], json!(true), "{text}: {result}");
        let expected = json!({ "total": text, "note": text, "history": [text, "0"] });
        assert_eq!(result["structuredContent"], expected, "{text}");
        assert!(
            validator.is_valid(&result["structuredContent"]),
            "{text}: the result fails its own outputSchema"
        );
    }
    // What the implementation received is the exact `i64`: nothing rounded
    // `2^53 + 1` on the way in.
    let received: Vec<i64> = PINNED.iter().map(|(_, value)| *value).collect();
    assert_eq!(registry.seen(), received);

    // A `BigInt` inside a declared `type` takes the same path.
    let ledger = json!({ "total": "9223372036854775807", "note": null, "history": ["-1"] });
    let result = client
        .call("echoLedger", json!({ "ledger": ledger }), None)
        .await;
    assert_eq!(result["structuredContent"], ledger);
    assert_eq!(registry.seen().last(), Some(&i64::MAX));
}

#[tokio::test]
async fn a_json_number_is_refused_naming_the_argument_and_never_runs() {
    let registry = Registry::default();
    let mut client = serve(&registry);
    let cases = [
        (
            json!({ "total": 9_007_199_254_740_993_i64, "totals": [] }),
            "total",
        ),
        (json!({ "total": i64::MAX, "totals": [] }), "total"),
        (json!({ "total": i64::MIN, "totals": [] }), "total"),
        (json!({ "total": 0, "totals": [] }), "total"),
        (json!({ "total": 1.0, "totals": [] }), "total"),
        (json!({ "total": "1", "maybe": 7, "totals": [] }), "maybe"),
        (
            json!({ "total": "1", "totals": ["1", 9_007_199_254_740_993_i64] }),
            "totals[1]",
        ),
    ];
    for (arguments, field) in cases {
        let result = client.call("echoBig", arguments.clone(), None).await;
        let error = envelope(&result);
        assert_eq!(error["code"], "VALIDATION_ERROR", "{arguments}: {error}");
        let message = error["message"].as_str().unwrap();
        assert!(
            message.contains(&format!("`{field}`")),
            "{arguments}: {message}"
        );
    }
    let nested = json!({ "ledger": { "total": 5, "history": [] } });
    let result = client.call("echoLedger", nested, None).await;
    let message = envelope(&result)["message"].as_str().unwrap().to_owned();
    assert!(message.contains("`ledger.total`"), "{message}");
    assert_eq!(registry.seen(), Vec::<i64>::new(), "a refused call ran");

    // Positive control: the same tool runs for a string, so the witness is live.
    let ok = client
        .call("echoBig", json!({ "total": "5", "totals": [] }), None)
        .await;
    assert_ne!(ok["isError"], json!(true), "{ok}");
    assert_eq!(registry.seen(), [5]);
}

#[tokio::test]
async fn a_non_canonical_or_out_of_range_string_is_refused() {
    let registry = Registry::default();
    let mut client = serve(&registry);
    for text in [
        "+5",
        "007",
        "-0",
        " 1",
        "1 ",
        "",
        "-",
        "1.0",
        "1e3",
        "0x10",
        "9223372036854775808",
        "-9223372036854775809",
        "10000000000000000000",
    ] {
        let result = client
            .call("echoBig", json!({ "total": text, "totals": [] }), None)
            .await;
        let error = envelope(&result);
        assert_eq!(error["code"], "VALIDATION_ERROR", "{text:?}: {error}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("`total`"), "{text:?}: {message}");
    }
    assert_eq!(registry.seen(), Vec::<i64>::new(), "a refused call ran");
}
