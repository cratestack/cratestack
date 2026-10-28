//! cratestack#1243: an `AuthRefusal` from the provider reaches an MCP
//! client as it reaches a REST one (cratestack#1208): its status, its
//! envelope members and `details`, and its own `WWW-Authenticate`
//! challenges after the guard's RFC 9728 one. A plain provider error keeps
//! the answer it had before.

mod support;

use axum::Router;
use axum::body::Body;
use cratestack_mcp::{CredentialPresence, ProtectedResource, StreamableHttpServer};
use http::{Request, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::device::DeviceProvider;
use support::http_app::{APP_ORIGIN, RESOURCE, Reply, mount, send, tool_call};
use support::token::ISSUER;

const METADATA_URL: &str = "http://localhost/.well-known/oauth-protected-resource/mcp";

fn app(tools: &FakeTools, presence: CredentialPresence) -> Router {
    let server = StreamableHttpServer::builder(
        tools.clone(),
        DeviceProvider::default(),
        [APP_ORIGIN],
        ProtectedResource::new(RESOURCE, [ISSUER]),
    )
    .with_credential_presence(presence)
    .build()
    .expect("valid configuration");
    mount(&server)
}

fn refused_call(case: &str, token: Option<&str>) -> Request<Body> {
    let mut request = tool_call("/mcp", token, "echo", json!({ "text": "hi" }));
    request
        .headers_mut()
        .insert("x-refusal", case.parse().unwrap());
    request
}

fn challenges(reply: &Reply) -> Vec<&str> {
    reply
        .headers
        .get_all("www-authenticate")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect()
}

/// Under the default rule (a bearer token present): the refusal's member
/// and challenge reach the client, after the guard's `invalid_token`
/// challenge.
#[tokio::test]
async fn a_401_refusal_keeps_its_members_and_challenges() {
    let tools = FakeTools::default();

    let reply = send(
        &app(&tools, CredentialPresence::bearer()),
        refused_call("renew", Some("opaque")),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        reply.json(),
        json!({
            "code": "UNAUTHORIZED",
            "message": "attestation expired",
            "details": null,
            "error": "attestation_expired",
        })
    );
    assert_eq!(
        challenges(&reply),
        [
            format!("Bearer error=\"invalid_token\", resource_metadata=\"{METADATA_URL}\""),
            "Signature error=\"attestation_expired\"".to_owned(),
        ]
    );
    assert_eq!(tools.runs(), 0);
}

/// A 403 from a provider that reads no bearer token: the status, the
/// `details` and the member, and a bare RFC 9728 challenge (no
/// `insufficient_scope`, which would be about a token never sent) before
/// the provider's own.
#[tokio::test]
async fn a_403_refusal_without_a_bearer_token_keeps_its_status_and_details() {
    let tools = FakeTools::default();

    let reply = send(
        &app(&tools, CredentialPresence::provider_decides()),
        refused_call("revoked", None),
    )
    .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    assert_eq!(
        reply.json(),
        json!({
            "code": "FORBIDDEN",
            "message": "device revoked",
            "details": { "device": "d-1" },
            "error": "DEVICE_REVOKED",
        })
    );
    assert_eq!(
        challenges(&reply),
        [
            format!("Bearer resource_metadata=\"{METADATA_URL}\""),
            "Signature error=\"device_revoked\"".to_owned(),
        ]
    );
    assert_eq!(tools.runs(), 0);
}

/// A plain `Unauthorized` reads exactly as before: the guard's fixed
/// message, no member, one challenge, and the provider's detail (which
/// names a secret here) stays in the log.
#[tokio::test]
async fn a_plain_provider_error_is_unchanged() {
    let tools = FakeTools::default();

    let reply = send(
        &app(&tools, CredentialPresence::bearer()),
        refused_call("plain", Some("opaque")),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        reply.json(),
        json!({
            "code": "UNAUTHORIZED",
            "message": "a valid bearer access token is required",
            "details": null,
        })
    );
    assert_eq!(
        challenges(&reply),
        [format!(
            "Bearer error=\"invalid_token\", resource_metadata=\"{METADATA_URL}\""
        )]
    );
}
