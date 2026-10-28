//! cratestack#1243: which requests reach the provider. By default only a
//! request with `Authorization: Bearer` does, exactly as before
//! `CredentialPresence` existed; an application whose provider reads
//! another credential (here `x-device`, standing in for a device signature)
//! lets the provider decide, or names its own test.

mod support;

use axum::Router;
use axum::body::Body;
use cratestack_mcp::{CredentialPresence, ProtectedResource, StreamableHttpServer};
use http::{Request, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::device::DeviceProvider;
use support::http_app::{APP_ORIGIN, RESOURCE, mount, send, tool_call};
use support::token::ISSUER;

const METADATA_URL: &str = "http://localhost/.well-known/oauth-protected-resource/mcp";

fn app(
    tools: &FakeTools,
    provider: &DeviceProvider,
    presence: Option<CredentialPresence>,
) -> Router {
    let mut builder = StreamableHttpServer::builder(
        tools.clone(),
        provider.clone(),
        [APP_ORIGIN],
        ProtectedResource::new(RESOURCE, [ISSUER]),
    );
    if let Some(presence) = presence {
        builder = builder.with_credential_presence(presence);
    }
    mount(&builder.build().expect("valid configuration"))
}

fn device_call(device: Option<&str>) -> Request<Body> {
    let mut request = tool_call("/mcp", None, "echo", json!({ "text": "hi" }));
    if let Some(device) = device {
        request
            .headers_mut()
            .insert("x-device", device.parse().unwrap());
    }
    request
}

/// The default is unchanged: a credential the provider would accept is
/// still refused before the provider runs when it is not a bearer token,
/// with the bare RFC 9728 challenge.
#[tokio::test]
async fn by_default_a_request_without_a_bearer_token_never_reaches_the_provider() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());

    let reply = send(&app(&tools, &provider, None), device_call(Some("d-1"))).await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        reply.header("www-authenticate"),
        format!("Bearer resource_metadata=\"{METADATA_URL}\"")
    );
    assert_eq!(reply.json()["code"], "UNAUTHORIZED");
    assert_eq!(provider.calls(), 0, "refused before the provider");
    assert_eq!(tools.runs(), 0);
}

/// The decisive test: no `Authorization` header at all, and the call runs
/// under the identity the provider read from its own credential.
#[tokio::test]
async fn when_the_provider_decides_a_request_without_authorization_can_authenticate() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());
    let app = app(
        &tools,
        &provider,
        Some(CredentialPresence::provider_decides()),
    );

    let reply = send(&app, device_call(Some("d-1"))).await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.result()["structuredContent"], json!({ "text": "hi" }));
    assert_eq!((provider.calls(), tools.runs()), (1, 1));
}

/// A provider that finds no credential returns no identity: still a 401
/// with the RFC 9728 challenge, and without an `error`, since the client
/// sent no bearer token for it to be about.
#[tokio::test]
async fn when_the_provider_decides_no_identity_is_the_bare_challenge() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());
    let app = app(
        &tools,
        &provider,
        Some(CredentialPresence::provider_decides()),
    );

    let reply = send(&app, device_call(None)).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        reply.header("www-authenticate"),
        format!("Bearer resource_metadata=\"{METADATA_URL}\"")
    );

    // With a bearer token the provider did not accept, the challenge says
    // so, as it does under the default rule.
    let mut request = device_call(None);
    request
        .headers_mut()
        .insert("authorization", "Bearer opaque".parse().unwrap());
    let reply = send(&app, request).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reply.header("www-authenticate"),
        format!("Bearer error=\"invalid_token\", resource_metadata=\"{METADATA_URL}\"")
    );
    assert_eq!(tools.runs(), 0);
}

/// A predicate decides before the body is read: a request it finds
/// without a credential never reaches the provider.
#[tokio::test]
async fn a_predicate_decides_which_requests_reach_the_provider() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());
    let presence = CredentialPresence::when(|parts: &http::request::Parts| {
        parts.headers.contains_key("x-device")
    });
    let app = app(&tools, &provider, Some(presence));

    let reply = send(&app, device_call(None)).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        reply.header("www-authenticate"),
        format!("Bearer resource_metadata=\"{METADATA_URL}\"")
    );
    assert_eq!(provider.calls(), 0, "refused before the provider");

    let reply = send(&app, device_call(Some("d-1"))).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!((provider.calls(), tools.runs()), (1, 1));
}

/// Whatever the rule, an `Authorization` the guard will not guess at (two
/// of them) is still 400 `invalid_request`, before the provider runs.
#[tokio::test]
async fn two_authorization_headers_are_refused_whatever_the_rule() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());
    let app = app(
        &tools,
        &provider,
        Some(CredentialPresence::provider_decides()),
    );
    let mut request = device_call(Some("d-1"));
    for value in ["Bearer a", "Bearer b"] {
        request
            .headers_mut()
            .append("authorization", value.parse().unwrap());
    }

    let reply = send(&app, request).await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(
        reply
            .header("www-authenticate")
            .starts_with("Bearer error=\"invalid_request\"")
    );
    assert_eq!((provider.calls(), tools.runs()), (0, 0));
}
