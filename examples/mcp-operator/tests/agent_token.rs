//! A non-bearer credential on the MCP endpoint (cratestack#1245 story 8).
//!
//! The endpoint asks its provider whether a request carries a credential
//! (`CredentialPresence::ask_provider()`, ADR 0021 §4), so `X-Agent-Token`
//! opens it exactly as `Authorization: Bearer` does, and is refused the
//! same way. Needs Postgres, like `tests/serve.rs`.

mod support;

use cratestack::axum::http::{HeaderMap, StatusCode};
use cratestack::serde_json::json;
use mcp_operator_example::provider::AGENT_TOKEN;
use mcp_operator_example::token::{STDIO_AUDIENCE, mint};
use support::{KEY, RESOURCE, bearer, db_or_skip, envelope, http};

fn agent(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AGENT_TOKEN, token.parse().unwrap());
    headers
}

#[tokio::test]
async fn an_agent_token_opens_the_endpoint_and_is_refused_like_a_bearer_token() {
    let Some((_container, db)) = db_or_skip().await else {
        return;
    };
    let limit = json!({ "limit": 10 });

    // The same token, in either credential, reaches the same policy check.
    let member = mint(KEY, RESOURCE, "u-1", "member", 60);
    let (status, _, by_agent) = http(&db, agent(&member), "recent_posts", limit.clone()).await;
    assert_eq!(status, StatusCode::OK, "{by_agent}");
    let (_, _, by_bearer) = http(&db, bearer(&member), "recent_posts", limit.clone()).await;
    assert_eq!(
        envelope(&by_agent["result"]),
        envelope(&by_bearer["result"])
    );

    // A member's publish is refused by the procedure's `@allow`, as over
    // a bearer token.
    let (status, _, denied) = http(&db, agent(&member), "publish_post", json!({ "id": 3 })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(denied["result"]["isError"], json!(true), "{denied}");

    // A token for another audience is the provider's 401, on both.
    let foreign = mint(KEY, STDIO_AUDIENCE, "u-1", "editor", 60);
    let (by_agent, _, agent_answer) =
        http(&db, agent(&foreign), "recent_posts", limit.clone()).await;
    let (by_bearer, _, bearer_answer) =
        http(&db, bearer(&foreign), "recent_posts", limit.clone()).await;
    assert_eq!(by_agent, StatusCode::UNAUTHORIZED, "{agent_answer}");
    assert_eq!(by_bearer, StatusCode::UNAUTHORIZED, "{bearer_answer}");
    assert_eq!(agent_answer, bearer_answer);

    // Neither credential: MCP's own challenge, pointing at the metadata.
    let (status, challenge, _) = http(&db, HeaderMap::new(), "recent_posts", limit).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let challenge = challenge.expect("a WWW-Authenticate challenge");
    assert!(challenge.contains("resource_metadata="), "{challenge}");
}
