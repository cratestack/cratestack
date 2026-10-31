//! The one test suite every provider runs (cratestack#1245). A provider's
//! test file implements [`Fixture`] (how to sign as the user and as the
//! admin, and one credential it refuses) and calls [`suite`]; nothing below
//! knows which provider it is driving.

// Each test target uses all of `suite` but only the helpers its own
// provider needs; Rust checks dead code per target.
#![allow(dead_code)]

pub mod jws;
pub mod op;

use cratestack::AuthProvider;
use cratestack::axum::body::Body;
use cratestack_auth_conformance::{Canonical, Credentials, Expected, Transport, auth_conformance};
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

/// Who a request is signed as.
#[derive(Clone, Copy, Debug)]
pub enum Who {
    /// An ordinary caller: no `admin` role.
    User,
    /// A caller whose `roles` contains `admin`.
    Admin,
}

/// What a provider's test supplies.
pub trait Fixture: Clone + Send + Sync + 'static {
    /// The provider under test.
    type Provider: AuthProvider;
    /// A fresh provider.
    fn provider(&self) -> Self::Provider;
    /// The subject a call signed as `who` reads as.
    fn subject(&self, who: Who) -> String;
    /// The credential for exactly `canonical`, signed as `who`.
    fn sign(&self, who: Who, canonical: &Canonical) -> Credentials;
    /// A credential the provider refuses, and how that refusal reads.
    fn refused(&self, canonical: &Canonical) -> Credentials;
    /// The refusal [`Fixture::refused`] gets, on every transport.
    fn refusal(&self) -> Expected;
}

/// The conformance kit, then the example's own schema: the same assertions
/// for every provider.
pub async fn suite<F: Fixture>(fixture: F) {
    let user = fixture.clone();
    let refused = fixture.clone();
    auth_conformance(fixture.provider())
        .accepts(fixture.subject(Who::User), move |c| user.sign(Who::User, c))
        .refuses(
            "the fixture's refused credential",
            move |c| refused.refused(c),
            fixture.refusal(),
        )
        .run()
        .await;

    let app = auth_pluggable::app(fixture.provider());
    let call = |procedure: &'static str| procedure_call(procedure);

    let anonymous = send(&app, &call("/$procs/whoAmI"), Credentials::default()).await;
    assert_ne!(
        anonymous.0,
        StatusCode::OK,
        "an anonymous caller is refused: {anonymous:?}"
    );

    let who = call("/$procs/whoAmI");
    let (status, body) = send(&app, &who, fixture.sign(Who::User, &who)).await;
    assert_eq!(status, StatusCode::OK, "whoAmI as the user: {body}");
    assert_eq!(body["subject"], fixture.subject(Who::User));

    let report = call("/$procs/adminReport");
    let (status, body) = send(&app, &report, fixture.sign(Who::User, &report)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "adminReport as the user: {body}"
    );

    let (status, body) = send(&app, &report, fixture.sign(Who::Admin, &report)).await;
    assert_eq!(status, StatusCode::OK, "adminReport as the admin: {body}");
    assert_eq!(body["subject"], fixture.subject(Who::Admin));
}

/// A REST call of a procedure with no arguments.
pub fn procedure_call(path: &'static str) -> Canonical {
    Canonical {
        transport: Transport::Rest,
        mount: "",
        method: "POST",
        path,
        query: None,
        body: b"{}".to_vec(),
    }
}

/// Sends `canonical` to `app` with `credentials`.
pub async fn send(
    app: &cratestack::axum::Router,
    canonical: &Canonical,
    credentials: Credentials,
) -> (StatusCode, Value) {
    let mut request = Request::post(canonical.path)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(Body::from(canonical.body.clone()))
        .expect("a valid request");
    for (name, value) in &credentials.headers {
        request.headers_mut().append(name, value.clone());
    }
    request.extensions_mut().extend(credentials.extensions);
    let response = app.clone().oneshot(request).await.expect("infallible");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("a body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
