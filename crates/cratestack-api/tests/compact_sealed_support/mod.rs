//! Shared by `compact_client_sealed_{rest,rpc}.rs`: compact wire keys under
//! the COSE envelope (`docs/design/compact-wire-keys.md` §5.1). A generated
//! `db = None` router sits behind an `EnvelopeLayer` whose compact opt-in is
//! chosen per test, with two captures: one inside the layer (what the
//! handlers' side sees once the seal is opened) and one outside it (what
//! crossed the wire, in `Cratestack-Payload-Type`).
//!
//! `sealed_compact_suite!` is the REST/RPC-shared body of the tests, so the
//! two transports cannot drift apart (transport parity). The invoking file
//! provides `serve_sealed`, `client`, `echo` and `order`.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use cratestack::axum::Router;
use cratestack::axum::extract::Request;
use cratestack::axum::http::HeaderMap;
use cratestack::axum::middleware::{Next, from_fn};
use cratestack::axum::response::Response;
use cratestack::compact::COMPACT_CBOR;
use cratestack::envelope_layer::EnvelopeLayerBuilder;

/// `(request header, response header)` pairs, one per call served.
pub type Log = Arc<Mutex<Vec<(String, String)>>>;

#[derive(Default, Clone)]
pub struct Logs {
    /// `Content-Type` of the opened request and of the handler's answer.
    pub inner: Log,
    /// `Cratestack-Payload-Type` of the sealed request and sealed answer.
    pub wire: Log,
}

/// Which compact payload types the envelope layer opted in to.
#[derive(Clone, Copy)]
pub enum OptIn {
    /// Nothing beyond the default: an old server.
    Nothing,
    /// Compact both ways.
    Both,
    /// Compact answers only; a compact request is refused.
    ResponsesOnly,
}

pub fn opt_in(builder: EnvelopeLayerBuilder, opt: OptIn) -> EnvelopeLayerBuilder {
    const CBOR: &str = "application/cbor";
    match opt {
        OptIn::Nothing => builder,
        OptIn::Both => builder.payload_media_types([CBOR, COMPACT_CBOR], [CBOR, COMPACT_CBOR]),
        OptIn::ResponsesOnly => builder.payload_media_types([CBOR], [CBOR, COMPACT_CBOR]),
    }
}

fn header(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn recording(router: Router, log: Log, request: &'static str, response: &'static str) -> Router {
    router.layer(from_fn(move |req: Request, next: Next| {
        let log = log.clone();
        async move {
            let sent = header(req.headers(), request);
            let answer: Response = next.run(req).await;
            let got = header(answer.headers(), response);
            log.lock().unwrap().push((sent, got));
            answer
        }
    }))
}

/// `router` (the generated one, compact layer already inside) behind the
/// envelope layer, on a loopback listener.
pub async fn serve(
    router: Router,
    logs: &Logs,
    opt: OptIn,
    generated: impl FnOnce(
        cratestack::cose::CoseEnvelope,
        cratestack::envelope_layer::EnvelopeMode,
        &'static str,
    ) -> EnvelopeLayerBuilder,
) -> std::net::SocketAddr {
    let inner = recording(router, logs.inner.clone(), "content-type", "content-type");
    let layered = inner.layer(crate::cose_client_support::layer(
        crate::cose_client_support::Kind::Ed25519,
        |envelope, policy, audience| opt_in(generated(envelope, policy, audience), opt),
    ));
    let wire = recording(
        layered,
        logs.wire.clone(),
        "cratestack-payload-type",
        "cratestack-payload-type",
    );
    crate::cose_client_support::serve(wire).await
}

/// A signing Ed25519 client for `addr`.
pub fn sealed_runtime(addr: std::net::SocketAddr) -> cratestack_client_rust::CratestackClient {
    crate::cose_client_support::runtime(
        addr,
        crate::cose_client_support::client_envelope(
            crate::cose_client_support::Kind::Ed25519,
            crate::cose_client_support::AUDIENCE,
        ),
    )
}

/// The tests both transports run. The invoking file provides `serve_sealed`
/// (`OptIn -> (SocketAddr, Logs)`), `client(addr, compact)`, `echo(&client)`
/// (a round trip that asserts the decoded value), `try_echo`, `is_unsigned_415`
/// and `stream_refused`.
#[macro_export]
macro_rules! sealed_compact_suite {
    () => {
        const C: &str = cratestack::compact::COMPACT_CBOR;

        fn pairs(log: &$crate::compact_sealed_support::Log) -> Vec<(String, String)> {
            log.lock().unwrap().clone()
        }

        fn pair(a: &str, b: &str) -> (String, String) {
            (a.to_owned(), b.to_owned())
        }

        #[tokio::test]
        async fn a_sealed_client_learns_compact_from_the_answer_then_seals_compact_requests() {
            let (addr, logs) = serve_sealed($crate::compact_sealed_support::OptIn::Both).await;
            let client = client(addr, true);
            echo(&client).await;
            echo(&client).await;
            // The handlers' side: named request, then compact both ways.
            assert_eq!(
                pairs(&logs.inner),
                [pair("application/cbor", C), pair(C, C)]
            );
            // On the wire: the first request names no payload type (CBOR),
            // both answers are sealed under the compact type, and the
            // second request is sealed under it too.
            assert_eq!(pairs(&logs.wire), [pair("", C), pair(C, C)]);
        }

        #[tokio::test]
        async fn a_sealed_client_stays_named_against_a_layer_that_did_not_opt_in() {
            let (addr, logs) = serve_sealed($crate::compact_sealed_support::OptIn::Nothing).await;
            let client = client(addr, true);
            for _ in 0..3 {
                echo(&client).await;
            }
            let cbor = pair("application/cbor", "application/cbor");
            assert_eq!(pairs(&logs.inner), [cbor.clone(), cbor.clone(), cbor]);
            // Never a compact request, so never a `415` to loop on.
            assert!(pairs(&logs.wire).iter().all(|(sent, _)| sent.is_empty()));
        }

        #[tokio::test]
        async fn a_refused_compact_request_turns_the_op_back_to_named() {
            let (addr, logs) =
                serve_sealed($crate::compact_sealed_support::OptIn::ResponsesOnly).await;
            let client = client(addr, true);
            echo(&client).await;
            // The op answered compact, so the second request is sealed
            // compact; this server does not take it: an unsigned `415`.
            let refused = try_echo(&client).await;
            assert!(is_unsigned_415(&refused), "{refused:?}");
            // The next call goes named and succeeds; nothing is retried.
            echo(&client).await;
            // The wire saw the refused compact request (unsigned answer, so
            // no payload type back) and then a named one; the handlers saw
            // only the two that were let through.
            assert_eq!(pairs(&logs.wire), [pair("", C), pair(C, ""), pair("", C)]);
            assert_eq!(pairs(&logs.inner).len(), 2);
        }

        #[tokio::test]
        async fn a_signed_stream_is_still_refused_and_sends_nothing() {
            let (addr, logs) = serve_sealed($crate::compact_sealed_support::OptIn::Both).await;
            let client = client(addr, true);
            assert!(stream_refused(&client).await);
            assert!(pairs(&logs.wire).is_empty());
        }
    };
}
