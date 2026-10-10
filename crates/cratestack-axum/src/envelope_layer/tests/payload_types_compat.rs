//! A peer that knows nothing of the payload-type headers (0.15.3) is
//! unchanged (cratestack#1168): the AAD the layer rebuilds for its request
//! and for its response are the bytes 0.15.3 produced, pinned as hex.

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, ResponseBinding};
use cratestack_cose::{CoseEnvelope, external_aad, request_digest};
use http::{Method, StatusCode};

use super::contracts_support::rest_under;
use super::fixtures::Hits;
use super::support::*;
use crate::envelope_layer::{OpenedRequest, SealContext, Sealed, ServerEnvelope, async_trait};

/// The AAD of `POST /widgets` for audience `payments`, contract `[7; 32]`,
/// no query, no bound headers, payload type `application/cbor`: captured
/// from the layer as it was before the payload type became negotiable.
const GOLDEN_REQUEST_AAD: &str = "8902687061796d656e747364504f5354682f7769646765747380f658200707070707070707070707070707070707070707070707070707070707070707706170706c69636174696f6e2f63626f7282f6f6";

#[derive(Default)]
struct Recorded {
    requests: Vec<Vec<u8>>,
    responses: Vec<Vec<u8>>,
}

/// The COSE envelope, recording the AAD of every binding it is handed.
struct Recording {
    inner: CoseEnvelope,
    seen: Arc<Mutex<Recorded>>,
}

#[async_trait]
impl ServerEnvelope for Recording {
    fn media_type(&self) -> &'static str {
        ServerEnvelope::media_type(&self.inner)
    }

    fn is_envelope_content_type(&self, content_type: &str) -> bool {
        ServerEnvelope::is_envelope_content_type(&self.inner, content_type)
    }

    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError> {
        ServerEnvelope::open_request(&self.inner, body, bind).await
    }

    async fn open_request_any(
        &self,
        body: Bytes,
        binds: &[Binding<'_>],
    ) -> Result<(OpenedRequest, usize), CratestackError> {
        let opened = ServerEnvelope::open_request_any(&self.inner, body, binds).await?;
        let aad = external_aad(&binds[opened.1]).expect("aad");
        self.seen.lock().expect("lock").requests.push(aad);
        Ok(opened)
    }

    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
        context: &SealContext,
    ) -> Result<Sealed, CratestackError> {
        let aad = external_aad(bind).expect("aad");
        self.seen.lock().expect("lock").responses.push(aad);
        ServerEnvelope::seal_response(&self.inner, payload, bind, context).await
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test]
async fn a_request_with_neither_header_binds_cbor_both_ways_exactly_as_0_15_3_did() {
    let seen = Arc::new(Mutex::new(Recorded::default()));
    let recording = Recording {
        inner: server_envelope(),
        seen: seen.clone(),
    };

    let hits = Hits::default();
    let router = rest_under(recording, CONTRACTS, None, &hits);
    let call = Call::new(Method::POST, "/widgets", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router,
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body.clone())
        .await
        .expect("a client that binds application/cbor verifies the answer");

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.requests.len(), 1);
    assert_eq!(seen.responses.len(), 1);
    assert_eq!(hex(&seen.requests[0]), GOLDEN_REQUEST_AAD);
    // The response AAD is the request's with the digest and the status
    // appended; the client rebuilds it from the literal type string.
    let expected = Binding {
        payload_media_type: "application/cbor".into(),
        response: Some(ResponseBinding {
            request: request_digest(&sealed),
            status: 200,
        }),
        ..call.binding(None)
    };
    assert_eq!(seen.responses[0], external_aad(&expected).expect("aad"));
    // And the answer names its type, which a 0.15.3 client ignores.
    assert_eq!(answer.seen("cratestack-payload-type"), "application/cbor");
}
