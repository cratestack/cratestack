//! A model feed over `application/cbor-seq` keeps the CBOR forms of its
//! values (cratestack#1001): the event's `event_id` and a `Uuid` field are
//! 16-byte strings, a `Decimal` stays its exact text, exactly as in a unary
//! CBOR response. A feed encoded with the SSE binding's JSON codec instead
//! would carry text ids, which a CBOR client built for byte strings (or the
//! other way round) cannot read: these tests fail if the REST or RPC model
//! handler serves `cbor-seq` with the wrong codec.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha").

use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};

#[path = "subscription_cbor_seq_shapes/common.rs"]
mod common;
#[path = "subscription_cbor_seq_shapes/rest.rs"]
mod rest;
#[path = "subscription_cbor_seq_shapes/rpc.rs"]
mod rpc;
mod support;

/// `x-caller: <id>` authenticates as that id; no header is anonymous; any
/// other value is refused (`401`), for the tests of a refusal's headers.
#[derive(Clone)]
struct HeaderCaller;

impl AuthProvider for HeaderCaller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let caller = request
            .headers
            .get("x-caller")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok());
        let refused = request.headers.contains_key("x-caller") && caller.is_none();
        core::future::ready(if refused {
            Err(CratestackError::Unauthorized("bad credential".to_owned()))
        } else {
            Ok(match caller {
                Some(id) => CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]),
                None => CratestackContext::anonymous(),
            })
        })
    }
}
