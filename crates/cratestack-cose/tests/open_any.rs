//! `CoseEnvelope::open_request_any` is public, so its two preconditions are
//! enforced, not documented (review of cratestack#1132, S4): an empty
//! candidate list is refused before anything is parsed or resolved, and
//! candidates that differ in anything but the op-contract digest are local
//! misuse, never a widened acceptance.

mod common;

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use common::{AUDIENCE, IAT, rpc_request};
use cratestack_core::{Binding, BoundHeaders, CratestackError, InMemoryNonceStore, PathParams};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseVerifierResolver, CoseVerifyKey, UNAUTHENTICATED,
};

struct Counting(AtomicUsize);

#[async_trait::async_trait]
impl CoseVerifierResolver for Counting {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        common::resolver().resolve(kid, alg).await
    }
}

fn server(resolver: Arc<dyn CoseVerifierResolver>) -> CoseEnvelope {
    common::server_with(
        CoseAlg::Ed25519,
        IAT,
        resolver,
        Arc::new(InMemoryNonceStore::new()),
    )
}

fn other_digest(bind: &Binding<'static>) -> Binding<'static> {
    Binding {
        contract_sha: common::other_contract_sha(),
        ..bind.clone()
    }
}

#[tokio::test]
async fn an_empty_candidate_list_is_the_coarse_401_and_touches_neither_parser_nor_resolver() {
    let counter = Arc::new(Counting(AtomicUsize::new(0)));
    let resolver: Arc<dyn CoseVerifierResolver> = counter.clone();
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rpc_request()).await;
    for body in [sealed, Bytes::from_static(b"not cose at all")] {
        match server(resolver.clone()).open_request_any(body, &[]).await {
            Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        0,
        "the resolver was called"
    );
}

#[tokio::test]
async fn candidates_differing_only_in_the_digest_still_open() {
    let sealed = common::sealed_request(CoseAlg::Ed25519, &other_digest(&rpc_request())).await;
    let binds = [rpc_request(), other_digest(&rpc_request())];
    let (_, index) = server(common::resolver())
        .open_request_any(sealed, &binds)
        .await
        .expect("the second digest verifies");
    assert_eq!(index, 1);
}

#[tokio::test]
async fn candidates_differing_in_anything_else_are_misuse() {
    let base = rpc_request();
    let variants: Vec<(&str, Binding<'static>)> = vec![
        (
            "audience",
            Binding {
                audience: Cow::Borrowed("ledger"),
                ..base.clone()
            },
        ),
        (
            "method",
            Binding {
                method: Cow::Borrowed("PUT"),
                ..base.clone()
            },
        ),
        (
            "route",
            Binding {
                route: Cow::Borrowed("model.Payment.list"),
                ..base.clone()
            },
        ),
        (
            "path_params",
            Binding {
                path_params: PathParams::Borrowed(&["x"]),
                ..base.clone()
            },
        ),
        (
            "query",
            Binding {
                query: Some(Cow::Borrowed("a=1")),
                ..base.clone()
            },
        ),
        (
            "payload_media_type",
            Binding {
                payload_media_type: Cow::Borrowed("application/json"),
                ..base.clone()
            },
        ),
        (
            "bound_headers",
            Binding {
                bound_headers: BoundHeaders {
                    idempotency_key: Some(Cow::Borrowed("k")),
                    if_match: None,
                },
                ..base.clone()
            },
        ),
    ];
    assert_eq!(base.audience, AUDIENCE);
    let sealed = common::sealed_request(CoseAlg::Ed25519, &base).await;
    for (what, other) in variants {
        let result = server(common::resolver())
            .open_request_any(sealed.clone(), &[base.clone(), other])
            .await;
        assert!(
            matches!(result, Err(CratestackError::Internal(_))),
            "{what}: {result:?}"
        );
    }
}
