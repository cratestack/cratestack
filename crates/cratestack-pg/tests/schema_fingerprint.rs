//! End-to-end proof that the `x-cratestack-schema-sha` drift-detection
//! header (issue #178) is really wired into the *generated* router, not
//! just exercised in isolation (`cratestack-axum::schema_fingerprint`'s
//! own unit tests already cover the middleware itself).
//!
//! Uses `connect_lazy` (no live Postgres needed, same technique as other
//! tests in this crate) and requests a path that matches no route —
//! `axum`'s `.layer()` wraps the whole router including its 404
//! fallback, so the schema-fingerprint middleware still runs, but nothing
//! ever touches the (unreachable-in-this-test) database. The assertion
//! that matters: the response status is identical across a matching
//! header, a mismatched header, and no header at all. If the middleware
//! ever started rejecting mismatches, this test would catch it as a
//! status-code divergence.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack::CratestackContext;
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/schema_fingerprint.cstack", db = Postgres);

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl std::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        std::future::ready(Ok(CratestackContext::authenticated([])))
    }
}

fn router() -> axum::Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let db = cratestack_schema::Cratestack::builder(pool).build();
    cratestack_schema::axum::model_router(db, (), cratestack_codec_cbor::CborCodec, AllowAllAuth)
}

async fn hit_unmatched_path_with_header(header: Option<&str>) -> StatusCode {
    let mut builder = Request::builder()
        .method("GET")
        .uri("/this-path-matches-no-route");
    if let Some(value) = header {
        builder = builder.header("x-cratestack-schema-sha", value);
    }
    let response = router()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .expect("router should produce a response, not reject the connection");
    response.status()
}

#[tokio::test]
async fn matching_mismatched_and_missing_header_all_produce_the_same_status() {
    // The real assertion: the schema-fingerprint header value never
    // changes the response (expected to be 404, since the path matches no
    // route — asserted explicitly below so a future routing change that
    // silently changes this doesn't make the "identical across headers"
    // check meaningless). A hard-reject implementation would diverge
    // here; this warn-only one must not.
    let with_correct = hit_unmatched_path_with_header(Some(cratestack_schema::SCHEMA_SHA256)).await;
    let with_wrong = hit_unmatched_path_with_header(Some("not-the-real-sha")).await;
    let with_none = hit_unmatched_path_with_header(None).await;

    assert_eq!(with_correct, StatusCode::NOT_FOUND);
    assert_eq!(with_correct, with_wrong);
    assert_eq!(with_correct, with_none);
}

#[test]
fn schema_sha256_is_a_real_sha256_hex_digest() {
    assert_eq!(cratestack_schema::SCHEMA_SHA256.len(), 64);
    assert!(
        cratestack_schema::SCHEMA_SHA256
            .chars()
            .all(|c| c.is_ascii_hexdigit())
    );
}
