//! cratestack#1243, ADR 0021 §4: `CredentialPresence::ask_provider()` lets
//! the provider's own `AuthProvider::credential` decide, before the body is
//! read, whether a request carries a credential. `x-device` stands in for a
//! device signature (see `support::device`).

mod support;

use axum::Router;
use axum::body::Body;
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, first_success,
};
use cratestack_mcp::{CredentialPresence, ProtectedResource, StreamableHttpServer};
use http::{Request, StatusCode};
use serde_json::json;
use support::FakeTools;
use support::device::DeviceProvider;
use support::http_app::{APP_ORIGIN, RESOURCE, mount, send, tool_call};
use support::token::ISSUER;

const METADATA_URL: &str = "http://localhost/.well-known/oauth-protected-resource/mcp";

fn app<A: AuthProvider>(tools: &FakeTools, provider: A) -> Router {
    let builder = StreamableHttpServer::builder(
        tools.clone(),
        provider,
        [APP_ORIGIN],
        ProtectedResource::new(RESOURCE, [ISSUER]),
    )
    .with_credential_presence(CredentialPresence::ask_provider());
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

/// `Absent`: the bare RFC 9728 challenge, and the provider is never called.
#[tokio::test]
async fn an_absent_credential_is_the_challenge_before_the_provider() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());

    let reply = send(&app(&tools, provider.clone()), device_call(None)).await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(
        reply.header("www-authenticate"),
        format!("Bearer resource_metadata=\"{METADATA_URL}\"")
    );
    assert_eq!((provider.calls(), tools.runs()), (0, 0));
}

/// `Present`: the provider runs, with no `Authorization` header at all.
#[tokio::test]
async fn a_present_credential_reaches_the_provider() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());

    let reply = send(&app(&tools, provider.clone()), device_call(Some("d-1"))).await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.result()["structuredContent"], json!({ "text": "hi" }));
    assert_eq!((provider.calls(), tools.runs()), (1, 1));
}

/// `Malformed`: 400 `invalid_request`, before the provider.
#[tokio::test]
async fn a_malformed_credential_is_invalid_request() {
    let (tools, provider) = (FakeTools::default(), DeviceProvider::default());

    let reply = send(
        &app(&tools, provider.clone()),
        device_call(Some("malformed")),
    )
    .await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(
        reply
            .header("www-authenticate")
            .starts_with("Bearer error=\"invalid_request\"")
    );
    assert_eq!((provider.calls(), tools.runs()), (0, 0));
}

/// A provider that does not say (`Unknown`, the trait's default) is always
/// asked, as under `provider_decides`.
#[tokio::test]
async fn a_provider_that_does_not_say_is_always_called() {
    let tools = FakeTools::default();
    let reply = send(&app(&tools, Silent), device_call(Some("d-1"))).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(tools.runs(), 1);
}

/// A first-success chain answers for its members: present when the device
/// member's credential is, so the chain reaches MCP without a bearer token.
#[tokio::test]
async fn a_chain_answers_for_its_members() {
    let (tools, device) = (FakeTools::default(), DeviceProvider::default());
    let chain = first_success(BearerOnly).or(device.clone());

    let reply = send(&app(&tools, chain.clone()), device_call(None)).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.text);
    assert_eq!(device.calls(), 0);

    let reply = send(&app(&tools, chain), device_call(Some("d-1"))).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!((device.calls(), tools.runs()), (1, 1));
}

/// Keeps the default `credential` and accepts every request as `silent`.
#[derive(Clone)]
struct Silent;

impl AuthProvider for Silent {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack_core::Value::String("silent".to_owned()),
        )]))
    }
}

/// A bearer-token provider that recognises no token: its credential is
/// `Credential::bearer`.
#[derive(Clone)]
struct BearerOnly;

impl AuthProvider for BearerOnly {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::anonymous())
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> cratestack_core::Credential<'r> {
        cratestack_core::Credential::bearer(request.headers)
    }
}
