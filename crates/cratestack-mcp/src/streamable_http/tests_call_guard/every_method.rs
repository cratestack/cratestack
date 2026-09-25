//! No method answers below the guard, in any protocol version a client can
//! name (phase-5 remediation review, cratestack#1040).
//!
//! The overrides in `server.rs` cover the methods whose answer is data. The
//! rest are `rmcp`'s defaults, and two of those answer on the lifecycle
//! older than 2026-07-28 without asking who is calling: `ping` with `{}`,
//! and `initialize` with `get_info()`, the server's name, version and
//! capabilities (`rmcp` 3.4.1, `handler/server.rs`). Neither is reachable
//! today, and each for a reason set elsewhere:
//!
//! - `with_stateless_protocol_metadata_required(true)` (`builder.rs`) makes
//!   `rmcp` refuse a request without an `MCP-Protocol-Version` header and a
//!   `_meta` version. Without it, a request naming no version is read as
//!   2025-03-26, and `ping` answers.
//! - `SUPPORTED` (`server/discovery.rs`) holds 2026-07-28 alone, so `rmcp`
//!   refuses a request whose `_meta` names an older version, and
//!   `initialize` has no version to agree on. Widen it, and both answer.
//!
//! On 2026-07-28 itself, `ping` and every method this server does not
//! implement are method-not-found. So this sends every method `rmcp`'s
//! server dispatch knows, below the guard without its caller, in each
//! version shape, and requires an error every time. A change to either
//! setting that lets one answer fails here, not in production.

use bytes::Bytes;
use http_body_util::Full;
use serde_json::{Value, json};

use super::{answer, server, user};
use crate::streamable_http::caller::hand_over;

/// Every request `rmcp` 3.4.1's `ClientRequest` holds, plus one it does
/// not (a custom method), with the smallest params each parses.
const METHODS: [(&str, &str); 19] = [
    (
        "initialize",
        r#"{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}"#,
    ),
    ("server/discover", "{}"),
    ("ping", "{}"),
    (
        "completion/complete",
        r#"{"ref":{"type":"ref/prompt","name":"p"},"argument":{"name":"a","value":"b"}}"#,
    ),
    ("logging/setLevel", r#"{"level":"debug"}"#),
    ("prompts/get", r#"{"name":"p"}"#),
    ("prompts/list", "{}"),
    ("resources/list", "{}"),
    ("resources/templates/list", "{}"),
    ("resources/read", r#"{"uri":"cratestack://x/y/1"}"#),
    ("subscriptions/listen", r#"{"notifications":{}}"#),
    ("resources/subscribe", r#"{"uri":"cratestack://x/y/1"}"#),
    ("resources/unsubscribe", r#"{"uri":"cratestack://x/y/1"}"#),
    ("tools/call", r#"{"name":"whoami","arguments":{}}"#),
    ("tools/list", "{}"),
    ("tasks/get", r#"{"taskId":"t"}"#),
    ("tasks/update", r#"{"taskId":"t","inputResponses":{}}"#),
    ("tasks/cancel", r#"{"taskId":"t"}"#),
    ("custom/method", "{}"),
];

/// `version`: the `MCP-Protocol-Version` header and the `_meta` version
/// both, or neither. `Mcp-Name` mirrors the body wherever `rmcp` requires
/// it, so a refusal is never the header check's.
fn raw(method: &str, params: &str, version: Option<&str>) -> http::Request<Full<Bytes>> {
    let mut params: Value = serde_json::from_str(params).unwrap();
    let mut builder = http::Request::post("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-method", method);
    if !matches!(method, "server/discover" | "subscriptions/listen") {
        for key in ["name", "uri", "taskId"] {
            if let Some(name) = params[key].as_str() {
                builder = builder.header("mcp-name", name);
            }
        }
    }
    if let Some(version) = version {
        builder = builder.header("mcp-protocol-version", version);
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": version,
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
        });
    }
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    builder
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

#[tokio::test]
async fn no_method_answers_below_the_guard_in_any_protocol_version() {
    let http = server();
    let inner = &http.service().shared.inner;

    // The harness can see an answer: the same shape, with the caller.
    let (mut parts, body) = raw("server/discover", "{}", Some("2026-07-28")).into_parts();
    hand_over(&mut parts, user());
    let guarded = answer(inner.handle(http::Request::from_parts(parts, body)).await).await;
    assert!(guarded["result"].is_object(), "{guarded}");

    let versions = [
        None,
        Some("2024-11-05"),
        Some("2025-03-26"),
        Some("2025-06-18"),
        Some("2025-11-25"),
        Some("2026-07-28"),
    ];
    for (method, params) in METHODS {
        for version in versions {
            let reply = answer(inner.handle(raw(method, params, version)).await).await;
            assert!(
                reply.get("result").is_none() && reply["error"]["code"].is_i64(),
                "{method} ({version:?}) answered below the guard: {reply}"
            );
            if version == Some("2026-07-28") && method != "initialize" {
                // The caller check's refusal, or a method this server
                // does not implement; nothing in between.
                let code = &reply["error"]["code"];
                assert!(
                    *code == json!(-32603) || *code == json!(-32601),
                    "{method}: {reply}"
                );
            }
        }
    }
}
