//! cratestack#919 / #911: a schema split across files keeps the
//! `x-cratestack-schema-sha` drift detection working, with a digest
//! computed over the merged schema (design §10, §15).
//!
//! `server` is the split schema, `drifted` is the same schema with one
//! field added to the part, and `concat` is the split schema written in a
//! single file. Needs no database (`connect_lazy`, unmatched path).

use axum::body::Body;
use axum::http::Request;
use cratestack::CratestackContext;
use cratestack::include_client_schema;
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

mod support;
use support::tracing_capture::{capture_events, init_tracing};

mod server {
    use super::*;
    include_server_schema!(
        "tests/fixtures/multi_file_fingerprint/schema.cstack",
        db = Postgres
    );
}
mod drifted {
    use super::*;
    include_client_schema!("tests/fixtures/multi_file_fingerprint_drifted/schema.cstack");
}
mod concat {
    use super::*;
    include_client_schema!("tests/fixtures/multi_file_fingerprint_concatenated.cstack");
}

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
    let db = server::cratestack_schema::Cratestack::builder(pool).build();
    server::cratestack_schema::axum::model_router(
        db,
        (),
        cratestack_codec_cbor::CborCodec,
        AllowAllAuth,
    )
}

/// Every captured `schema SHA-256 differ` warning for one request that
/// carries `sha` in the drift header.
async fn drift_warnings_for(sha: &str) -> Vec<String> {
    init_tracing();
    let request = Request::builder()
        .uri("/this-path-matches-no-route")
        .header("x-cratestack-schema-sha", sha)
        .body(Body::empty())
        .unwrap();
    let (response, events) = capture_events(router().oneshot(request)).await;
    response.expect("router should produce a response");
    events
        .into_iter()
        .filter(|event| event.contains("expected_schema_sha"))
        .collect()
}

#[test]
fn splitting_does_not_change_schema_sha256() {
    assert_eq!(
        server::cratestack_schema::SCHEMA_SHA256,
        concat::cratestack_schema::SCHEMA_SHA256
    );
    assert_eq!(
        server::cratestack_schema::SCHEMA_SHA256_BYTES,
        concat::cratestack_schema::SCHEMA_SHA256_BYTES
    );
    assert_eq!(server::cratestack_schema::SCHEMA_FILES.len(), 2);
    assert!(concat::cratestack_schema::SCHEMA_SOURCE.contains("model Widget"));
}

#[test]
fn a_drifted_part_changes_schema_sha256() {
    assert_ne!(
        server::cratestack_schema::SCHEMA_SHA256,
        drifted::cratestack_schema::SCHEMA_SHA256
    );
    // The drifted root file is byte-identical to the original root, so
    // only the digest over the merged schema can see the change.
    assert_eq!(
        drifted::cratestack_schema::SCHEMA_SOURCE,
        server::cratestack_schema::SCHEMA_SOURCE
    );
}

#[tokio::test]
async fn the_server_warns_on_a_drifted_part_digest() {
    let warnings = drift_warnings_for(drifted::cratestack_schema::SCHEMA_SHA256).await;
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning: {warnings:?}"
    );
    assert!(warnings[0].contains(&format!(
        "expected_schema_sha={}",
        server::cratestack_schema::SCHEMA_SHA256
    )));
    assert!(warnings[0].contains(&format!(
        "received_schema_sha={}",
        drifted::cratestack_schema::SCHEMA_SHA256
    )));
}

#[tokio::test]
async fn the_server_is_silent_on_the_concatenated_digest() {
    let warnings = drift_warnings_for(concat::cratestack_schema::SCHEMA_SHA256).await;
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}
