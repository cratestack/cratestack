//! cratestack#1039: MCP behaviour behind the guard — SEP-2243 header
//! checks, the per-request identity reaching the tool, the body limit — and
//! L3 admission over HTTP, which must be stdio's exactly (ADR 0002 § Security
//! requirement 13): the same namespace per caller, the same replay, the same
//! `StoreErrorPolicy`.

mod support;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use cratestack_core::{CratestackError, IdempotencyStore, RateLimitConfig};
use cratestack_mcp::{OpExecutor, StoreErrorPolicy};
use http::StatusCode;
use serde_json::{Value, json};
use support::FakeTools;
use support::failing::FailingLimiter;
use support::http_app::{RESOURCE, builder, keyed_call, mount, post, rpc, send, served, token};
use support::stores::MemoryIdempotency;

/// A `tools/call` of `echo` whose `Mcp-Name` / `Mcp-Method` headers say
/// `name` / `method` instead of what the body says.
fn call_with_headers(name: &str, method: &str) -> http::Request<Body> {
    let body = rpc(
        "tools/call",
        json!({ "name": "echo", "arguments": { "text": "a" } }),
    );
    let mut request = post("/mcp", Some(&token("u-1")), &body)
        .body(Body::from(body.to_string()))
        .unwrap();
    let headers = request.headers_mut();
    headers.insert("mcp-name", name.parse().unwrap());
    headers.insert("mcp-method", method.parse().unwrap());
    request
}

/// Headers that disagree with the body are answered by `rmcp` with HTTP
/// 400 and JSON-RPC `-32020` (header mismatch), before the handler. The
/// guard passes the request through untouched apart from `Authorization`,
/// so this is proof that it does not bypass `rmcp`'s checks.
#[tokio::test]
async fn a_mismatched_mcp_method_or_name_is_400_header_mismatch() {
    let tools = FakeTools::default();
    let app = served(&tools);
    let control = send(&app, call_with_headers("echo", "tools/call")).await;
    assert_eq!(control.status, StatusCode::OK, "{}", control.text);
    for (name, method) in [("transfer", "tools/call"), ("echo", "tools/list")] {
        let request = call_with_headers(name, method);

        let reply = send(&app, request).await;

        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{name} {method}: {}",
            reply.text
        );
        assert_eq!(
            reply.json()["error"]["code"],
            json!(-32020),
            "{}",
            reply.text
        );
    }
    assert_eq!(tools.runs(), 1, "only the matching control ran");
}

/// The identity the provider built is the one the tool runs as. The fake
/// `transfer` echoes `ctx.principal_actor_id()`, so two tokens give two
/// answers. (The generated-table version, a procedure policy keyed on an
/// auth field, is `cratestack-api`'s `tests/mcp_http.rs`.)
#[tokio::test]
async fn each_request_runs_as_its_own_token() {
    let tools = FakeTools::default();
    let app = served(&tools);
    for (id, run) in [("u-1", 1), ("u-2", 2)] {
        let call = keyed_call(
            "/mcp",
            Some(&token(id)),
            "transfer",
            json!({ "amount": 3 }),
            None,
        );
        let result = send(&app, call).await.result();
        assert_eq!(
            result["content"][0]["text"],
            json!([3, run, id]).to_string(),
            "{result}"
        );
    }
}

#[tokio::test]
async fn a_body_over_the_limit_is_413_before_the_provider() {
    let tools = FakeTools::default();
    let huge = "x".repeat(4 * 1024 * 1024 + 1);
    let request = post("/mcp", Some(&token("u-1")), &rpc("tools/list", json!({})))
        .body(Body::from(huge))
        .unwrap();
    let reply = send(&served(&tools), request).await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
}

fn idempotent() -> OpExecutor {
    let store: Arc<dyn IdempotencyStore> = Arc::new(MemoryIdempotency::default());
    OpExecutor::new(Some(store), Duration::from_secs(60))
}

#[tokio::test]
async fn a_key_replays_over_http_and_stays_in_its_callers_namespace() {
    let tools = FakeTools::default();
    let server = builder(&tools, RESOURCE)
        .with_executor(idempotent())
        .build()
        .unwrap();
    let app = mount(&server);
    let call = |id: &str| {
        keyed_call(
            "/mcp",
            Some(&token(id)),
            "transfer",
            json!({ "amount": 5 }),
            Some("k-1"),
        )
    };

    let first = send(&app, call("u-1")).await.result();
    let again = send(&app, call("u-1")).await.result();
    let other = send(&app, call("u-2")).await.result();

    assert_eq!(tools.runs(), 2, "u-1's retry replays; u-2 runs its own");
    assert_eq!(first["content"], again["content"]);
    assert_eq!(
        again["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
    assert_eq!(
        other["content"][0]["text"],
        json!([5, 2, "u-2"]).to_string()
    );
}

/// `StoreErrorPolicy::Deny` refuses a call whose rate-limit store is
/// unavailable, on HTTP as on stdio; the default serves it.
#[tokio::test]
async fn the_store_error_policy_applies_over_http() {
    for (policy, refused) in [(Some(StoreErrorPolicy::Deny), true), (None, false)] {
        let tools = FakeTools::default();
        let limiter = Arc::new(FailingLimiter::new(|| {
            CratestackError::Unavailable("redis down".to_owned())
        }));
        let executor = OpExecutor::new(None, Duration::ZERO)
            .with_rate_limit(limiter, RateLimitConfig::new(5, 0.0));
        let mut configured = builder(&tools, RESOURCE).with_executor(executor);
        if let Some(policy) = policy {
            configured = configured.with_store_error_policy(policy);
        }
        let app = mount(&configured.build().unwrap());

        let call = keyed_call(
            "/mcp",
            Some(&token("u-1")),
            "echo",
            json!({ "text": "a" }),
            None,
        );
        let result = send(&app, call).await.result();

        assert_eq!(result["isError"], json!(refused), "{policy:?}: {result}");
        assert_eq!(tools.runs(), usize::from(!refused), "{policy:?}");
        if refused {
            let envelope: Value =
                serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(envelope["code"], "UNAVAILABLE", "{envelope}");
        }
    }
}
