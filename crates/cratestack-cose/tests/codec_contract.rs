//! A codec that breaks `CratestackCodec::encode_into`'s contract ("append,
//! leave what `out` already holds untouched") is local misuse, a `500`,
//! never a panic or a wrapped length (security review of cratestack#1005,
//! round 2). `seal_value` hands a third-party codec the envelope's own
//! buffer, with room reserved at the front for the headers.

mod common;

use common::{IAT, rpc_request};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackError};
use cratestack_cose::CoseAlg;
use serde::{Deserialize, Serialize};

/// How a codec mistreats the buffer it is handed.
#[derive(Clone, Copy)]
enum Abuse {
    /// Replaces it with its own `Vec` (the reviewer's probe): shorter than
    /// the reserved room for a short value.
    Replace,
    /// Clears it, then appends: the reserved room is gone.
    Truncate,
    /// Appends correctly, but also writes into the reserved room.
    ScribbleOnTheReservedRoom,
}

#[derive(Clone)]
struct Misbehaving(Abuse);

impl CratestackCodec for Misbehaving {
    const CONTENT_TYPE: &'static str = CborCodec::CONTENT_TYPE;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        CborCodec.encode(value)
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        CborCodec.decode(bytes)
    }

    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        match self.0 {
            Abuse::Replace => *out = self.encode(value)?,
            Abuse::Truncate => {
                out.clear();
                CborCodec.encode_into(value, out)?;
            }
            Abuse::ScribbleOnTheReservedRoom => {
                CborCodec.encode_into(value, out)?;
                out[0] = 0xff;
            }
        }
        Ok(())
    }
}

/// The reviewer's probe `a_codec_that_replaces_the_buffer_panics_the_sealer`,
/// inverted, and its two siblings. On c5c3f7a0 the replacing codec
/// panicked the sealer in a debug build ("attempt to subtract with
/// overflow" in `seal.rs`); a release build would have wrapped the length
/// and then panicked on an out-of-range slice.
#[tokio::test]
async fn a_codec_that_does_not_append_is_misuse_not_a_panic() {
    for abuse in [
        Abuse::Replace,
        Abuse::Truncate,
        Abuse::ScribbleOnTheReservedRoom,
    ] {
        for &alg in CoseAlg::ALL {
            let server = common::server(alg, IAT);
            let client = common::client(alg, IAT, common::CTI_16);
            let request = common::sealed_request(alg, &rpc_request()).await;
            let response = common::response_to(&rpc_request(), &request, 200);
            let codec = Misbehaving(abuse);
            // Spawned, so a panic surfaces as a `JoinError` the test can
            // report instead of aborting it.
            let sealed = tokio::spawn(async move {
                let as_response = server
                    .seal_response_value(&codec, &"short", &response)
                    .await;
                let as_request = client
                    .seal_request_value(&codec, &"short", &rpc_request())
                    .await;
                (as_response, as_request)
            })
            .await
            .expect("the sealer must not panic");
            for result in [sealed.0, sealed.1] {
                assert!(
                    matches!(&result, Err(CratestackError::Internal(message)) if message.contains("encode_into")),
                    "{alg:?}: {result:?}"
                );
            }
        }
    }
}
