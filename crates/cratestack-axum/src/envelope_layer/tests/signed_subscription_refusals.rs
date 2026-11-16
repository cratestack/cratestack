//! What a signed subscription open is refused with, and that the handler
//! never runs for it: a sealed `406` for a stream type that is not literal,
//! a `HEAD`, a subscription the schema does not mark, an empty table; and the
//! principal mapper's refusal.

use cratestack_core::request_digest;
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::signed_subscription_support::*;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayerBuilder, PrincipalMapper, VerifiedRequest, async_trait};

const COSE: &str = "application/cose; cose-type=\"cose-sign1\"";

async fn refused(
    builder: EnvelopeLayerBuilder,
    method: Method,
    op: &'static str,
    accept: &[&str],
) -> Hits {
    let hits = Hits::default();
    let router = router(builder.build().expect("layer"), &hits, Serve::Items);
    let (request, call, sealed) = open(method.clone(), op, accept).await;
    let answer = send(&router, request).await;
    let at = format!("{method} {op} {accept:?}");
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE, "{at}");
    assert!(answer.is_sealed(), "{at}: {}", answer.content_type());
    // A sealed `HEAD` answer is not opened by clients (its body is dropped
    // on the wire), as in the other `HEAD` suites.
    if method != Method::HEAD {
        call.open(request_digest(&sealed), answer.status, answer.body)
            .await
            .unwrap_or_else(|error| panic!("{at}: not sealed under the binding: {error}"));
    }
    hits
}

#[tokio::test]
async fn a_stream_type_that_is_not_literal_is_a_sealed_406() {
    for accept in [
        vec![COSE],
        vec!["*/*"],
        vec!["application/cbor-seq;q=0, */*"],
        vec!["text/*, application/cose"],
        vec!["TEXT/EVENT-STREAM"],
        // Two `Accept` headers are ambiguous: none is honoured.
        vec![CBOR_SEQ, COSE],
        vec![],
    ] {
        let hits = refused(builder(), Method::GET, MARKED, &accept).await;
        assert_eq!(hits.get(), 0, "{accept:?}: the handler must not run");
    }
}

#[tokio::test]
async fn a_sealed_head_on_a_subscription_is_a_sealed_406() {
    let hits = refused(builder(), Method::HEAD, MARKED, &[CBOR_SEQ]).await;
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn an_unmarked_or_unlisted_subscription_is_a_sealed_406_before_its_handler() {
    // Unmarked, with the stream asked for properly.
    for method in [Method::GET, Method::POST] {
        let hits = refused(builder(), method, UNMARKED, &[CBOR_SEQ]).await;
        assert_eq!(hits.get(), 0, "unmarked");
    }
    // The default, empty table: nothing may open signed (fail closed).
    let hits = refused(
        builder().subscriptions(&[]),
        Method::GET,
        MARKED,
        &[CBOR_SEQ],
    )
    .await;
    assert_eq!(hits.get(), 0, "empty table");
}

/// The bare RPC key matches any method, so only `is_subscription()` stops a
/// unary call to a marked op id from being taken for an open.
#[tokio::test]
async fn a_unary_call_to_a_marked_op_is_not_an_open() {
    let hits = Hits::default();
    let router = router(builder().build().expect("layer"), &hits, Serve::Items);
    let call = Call::new(Method::POST, MARKED, &[]);
    let sealed = call.seal(PAYLOAD).await;
    let request = cose_request(Method::POST, "/rpc/model.Widget.subscribe", sealed.clone());
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed(), "{}", answer.content_type());
    assert_eq!(answer.seen("cratestack-frames"), "<absent>");
}

struct Refusing;

#[async_trait]
impl PrincipalMapper for Refusing {
    async fn principal(
        &self,
        _: &VerifiedRequest<'_>,
    ) -> Result<String, cratestack_core::CratestackError> {
        Err(cratestack_core::CratestackError::Unauthorized(
            "no".to_owned(),
        ))
    }
}

/// A signer the mapper refuses is the coarse unsigned `401`, as for a unary
/// call, and the handler does not run.
#[tokio::test]
async fn a_signer_the_principal_mapper_refuses_is_the_unsigned_401() {
    let hits = Hits::default();
    let layer = builder().principal_mapper(Refusing).build().expect("layer");
    let router = router(layer, &hits, Serve::Items);
    let (request, ..) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
}

/// A replayed signed open is the coarse `401`, and does not stream twice.
#[tokio::test]
async fn a_replayed_open_is_the_coarse_401() {
    let hits = Hits::default();
    let router = router(builder().build().expect("layer"), &hits, Serve::Items);
    let (request, call, sealed) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let first = send(&router, request).await;
    assert_eq!(first.status, StatusCode::OK);
    let mut again = cose_request(Method::GET, MARKED_PATH, sealed);
    again.headers_mut().insert(
        http::header::ACCEPT,
        http::HeaderValue::from_static(CBOR_SEQ),
    );
    let second = send(&router, again).await;
    assert_eq!(second.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 1);
    let _ = call;
}
