use crate::client::helpers::headers_to_runtime;
use crate::runtime::wire::RuntimeHeader;
use cratestack_core::PAYLOAD_TYPE_HEADER;
use reqwest::header::{
    CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, TRANSFER_ENCODING,
};

/// The response's headers as the plain call would have seen them: the body
/// is the opened payload, so its framing headers (and the layer's own
/// `Cratestack-Payload-Type`) go and its type is the one it was sealed under.
/// Everything else (`ETag`, `Retry-After`, `Idempotency-Replayed`) is passed
/// on, and stays unauthenticated.
pub(super) fn opened_headers(headers: &HeaderMap, payload_type: &str) -> Vec<RuntimeHeader> {
    let dropped = |name: &str| {
        [
            CONTENT_TYPE.as_str(),
            CONTENT_LENGTH.as_str(),
            CONTENT_ENCODING.as_str(),
            TRANSFER_ENCODING.as_str(),
            PAYLOAD_TYPE_HEADER,
        ]
        .iter()
        .any(|dropped| name.eq_ignore_ascii_case(dropped))
    };
    let mut kept: Vec<RuntimeHeader> = headers_to_runtime(headers)
        .into_iter()
        .filter(|header| !dropped(&header.name))
        .collect();
    kept.push(RuntimeHeader {
        name: CONTENT_TYPE.as_str().to_owned(),
        value: payload_type.to_owned(),
    });
    kept
}
