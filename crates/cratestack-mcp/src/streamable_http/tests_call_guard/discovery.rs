//! `server/discover` and `completion/complete` fail closed without the
//! guard's caller too (maintainer decision on cratestack#1040). Neither
//! answer depends on who asks, which is why `rmcp`'s defaults answered them
//! whoever did; the decision makes them no exception to the rule every other
//! method follows. Same shape as the parent module's tests: the request goes
//! to the `rmcp` service behind the guard, and the same request carrying the
//! guard's caller must succeed, so the refusal is the caller check's.

use cratestack_core::{CratestackContext, CratestackError};
use serde_json::json;

use super::{Table, answer, request, server, user};
use crate::{ProtectedResource, StreamableHttpServer};

#[tokio::test]
async fn a_discover_that_skipped_the_guard_fails_closed() {
    let http = server();
    let inner = &http.service().shared.inner;

    let unguarded = request("server/discover", json!({}), None, None);
    let unguarded = answer(inner.handle(unguarded).await).await;
    assert_eq!(unguarded["error"]["code"], json!(-32603), "{unguarded}");
    assert_eq!(
        unguarded["error"]["message"], "no authenticated caller for this request",
        "{unguarded}"
    );

    let guarded = request("server/discover", json!({}), None, Some(user()));
    let guarded = answer(inner.handle(guarded).await).await;
    assert_eq!(
        guarded["result"]["supportedVersions"],
        json!(["2026-07-28"]),
        "{guarded}"
    );
    assert!(
        guarded["result"]["capabilities"]["tools"].is_object(),
        "{guarded}"
    );
}

/// The override adds the caller check and nothing else, so the answer is
/// `rmcp`'s default one whole: every field, `serverInfo` included. Nothing
/// else reads `with_implementation` or the `serverInfo` `get_info` sets, so
/// without this, either could be dropped with every suite green (phase-5
/// remediation review, which found exactly that when both moved to
/// `server/discovery.rs`).
#[tokio::test]
async fn an_authenticated_discover_is_rmcps_default_answer_whole() {
    let provider = |_: &http::HeaderMap| Ok::<_, CratestackError>(CratestackContext::anonymous());
    let resource = ProtectedResource::new("http://localhost/mcp", ["https://auth.example.test"]);
    let named =
        StreamableHttpServer::builder(Table, provider, ["https://app.example.test"], resource)
            .with_implementation("probe", "9.9.9")
            .build()
            .unwrap();
    let guarded = request("server/discover", json!({}), None, Some(user()));
    let guarded = answer(named.service().shared.inner.handle(guarded).await).await;
    assert_eq!(
        guarded["result"],
        json!({
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": { "tools": {} },
            "ttlMs": 0,
            "cacheScope": "private",
            "_meta": {
                "io.modelcontextprotocol/serverInfo": { "name": "probe", "version": "9.9.9" },
            },
        }),
        "{guarded}"
    );

    let http = server();
    let guarded = request("server/discover", json!({}), None, Some(user()));
    let guarded = answer(http.service().shared.inner.handle(guarded).await).await;
    assert_eq!(
        guarded["result"]["_meta"]["io.modelcontextprotocol/serverInfo"],
        json!({ "name": "cratestack-mcp", "version": env!("CARGO_PKG_VERSION") }),
        "{guarded}"
    );
}

#[tokio::test]
async fn a_completion_that_skipped_the_guard_fails_closed() {
    let params = json!({
        "ref": { "type": "ref/prompt", "name": "summary" },
        "argument": { "name": "topic", "value": "po" },
    });
    let http = server();
    let inner = &http.service().shared.inner;

    let unguarded = request("completion/complete", params.clone(), None, None);
    let unguarded = answer(inner.handle(unguarded).await).await;
    assert_eq!(unguarded["error"]["code"], json!(-32603), "{unguarded}");
    assert_eq!(
        unguarded["error"]["message"], "no authenticated caller for this request",
        "{unguarded}"
    );

    // Whole, as `discover`'s is: `rmcp`'s default, an empty completion.
    let guarded = request("completion/complete", params, None, Some(user()));
    let guarded = answer(inner.handle(guarded).await).await;
    assert_eq!(
        guarded["result"],
        json!({ "resultType": "complete", "completion": { "values": [] } }),
        "{guarded}"
    );
}
