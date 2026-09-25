//! `server/discover` and `completion/complete` fail closed without the
//! guard's caller too (maintainer decision on cratestack#1040). Neither
//! answer depends on who asks, which is why `rmcp`'s defaults answered them
//! whoever did; the decision makes them no exception to the rule every other
//! method follows. Same shape as the parent module's tests: the request goes
//! to the `rmcp` service behind the guard, and the same request carrying the
//! guard's caller must succeed, so the refusal is the caller check's.

use serde_json::json;

use super::{answer, request, server, user};

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

    let guarded = request("completion/complete", params, None, Some(user()));
    let guarded = answer(inner.handle(guarded).await).await;
    assert_eq!(
        guarded["result"]["completion"]["values"],
        json!([]),
        "{guarded}"
    );
}
