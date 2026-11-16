use super::choose;
use crate::envelope_error::EnvelopeError;
use crate::subscription::{FramesMode, SubscriptionTransport};

const RPC: &str = "subscribe/procedure.pulse";
const REST: &str = "/$procs/pulse";
const UNSIGNED: Option<FramesMode> = Some(FramesMode::Unsigned);

fn refused(result: Result<bool, EnvelopeError>) -> bool {
    matches!(result, Err(EnvelopeError::StreamsUnsupported))
}

#[test]
fn auto_and_cbor_seq_read_cbor_seq_and_sse_reads_sse() {
    for transport in [SubscriptionTransport::Auto, SubscriptionTransport::CborSeq] {
        assert!(matches!(choose(transport, UNSIGNED, RPC, false), Ok(false)));
    }
    assert!(matches!(
        choose(SubscriptionTransport::Sse, UNSIGNED, RPC, false),
        Ok(true)
    ));
}

#[test]
fn a_socket_pin_is_refused_and_auto_never_falls_back_to_one() {
    for transport in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::WebTransport,
    ] {
        assert!(refused(choose(transport, UNSIGNED, RPC, false)));
    }
}

#[test]
fn an_unmarked_subscription_is_refused_on_every_binding() {
    for transport in [
        SubscriptionTransport::Auto,
        SubscriptionTransport::CborSeq,
        SubscriptionTransport::Sse,
    ] {
        assert!(refused(choose(transport, None, RPC, false)));
    }
}

#[test]
fn a_rest_route_is_refused_on_wasm32_only() {
    // A REST open is a `GET` with a body, which `fetch` refuses.
    assert!(refused(choose(
        SubscriptionTransport::Auto,
        UNSIGNED,
        REST,
        true
    )));
    assert!(choose(SubscriptionTransport::Auto, UNSIGNED, REST, false).is_ok());
    assert!(choose(SubscriptionTransport::Auto, UNSIGNED, RPC, true).is_ok());
}
