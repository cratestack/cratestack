//! The payload-type selector headers of the signed transport
//! (ADR 0006 §4, cratestack#1168).
//!
//! A signed message carries its inner payload under one media type, bound
//! once in the AAD (`Binding::payload_media_type`) and never in a COSE
//! header. Which type that is, is negotiated with two **unbound selector
//! headers**, the same pattern as the `Cratestack-Contract` selector: they
//! select among types the verifier already allows, they never widen it, and
//! the AAD carries the whole type, so a header that lies about the payload
//! fails the signature with the ordinary coarse `401`.
//!
//! - [`PAYLOAD_TYPE_HEADER`] on a request: the type of the sealed request
//!   payload. On a sealed response: the type of the sealed response
//!   payload. Absent means [`DEFAULT_PAYLOAD_MEDIA_TYPE`] (`application/cbor`),
//!   so every peer that predates the header is unchanged.
//! - [`PAYLOAD_ACCEPT_HEADER`] on a request: the response payload types the
//!   client can read, in preference order. Absent means CBOR alone.
//!
//! **One grammar, no normalisation**, so a client and a server cannot
//! disagree: a payload type is a lowercase `type "/" subtype` of RFC 9110
//! `token` characters, no parameters, no wildcard, at most
//! [`MAX_PAYLOAD_TYPE_LEN`] bytes; an accept list is one to
//! [`MAX_PAYLOAD_ACCEPT_ENTRIES`] distinct such types joined by `", "`, with
//! no `q`. Whether a type may be *sealed* is a separate question
//! ([`is_sealable_payload_type`]): never an envelope inside an envelope, a
//! stream, or a multipart body.

use crate::error::CratestackError;

/// The request and response header that names the sealed payload's type.
pub const PAYLOAD_TYPE_HEADER: &str = "Cratestack-Payload-Type";

/// The request header that lists the response payload types the client reads.
pub const PAYLOAD_ACCEPT_HEADER: &str = "Cratestack-Payload-Accept";

/// What a message that names no payload type carries: CBOR.
pub const DEFAULT_PAYLOAD_MEDIA_TYPE: &str = "application/cbor";

/// The longest payload type, in bytes.
pub const MAX_PAYLOAD_TYPE_LEN: usize = 127;

/// The most types an accept list may name.
pub const MAX_PAYLOAD_ACCEPT_ENTRIES: usize = 8;

/// The `RpcErrorBody` code of the unsigned `415` a verifier answers when the
/// request names a payload type the op does not accept.
pub const PAYLOAD_TYPE_UNSUPPORTED_CODE: &str = "payload_type_unsupported";

/// [`PAYLOAD_TYPE_UNSUPPORTED_CODE`]'s REST twin.
pub const PAYLOAD_TYPE_UNSUPPORTED_REST_CODE: &str = "PAYLOAD_TYPE_UNSUPPORTED";

/// The `RpcErrorBody` code of the unsigned `406` a verifier answers when none
/// of the response types the client reads may be sealed for the op.
pub const PAYLOAD_TYPE_NOT_ACCEPTABLE_CODE: &str = "payload_type_not_acceptable";

/// [`PAYLOAD_TYPE_NOT_ACCEPTABLE_CODE`]'s REST twin.
pub const PAYLOAD_TYPE_NOT_ACCEPTABLE_REST_CODE: &str = "PAYLOAD_TYPE_NOT_ACCEPTABLE";

/// Whether `byte` is an RFC 9110 `tchar` that this grammar allows: a
/// lowercase letter, a digit, or one of ``!#$%&'+-.^_`|~``. Uppercase is
/// refused so a type has exactly one spelling, and `*` so no wildcard passes
/// as a type.
fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_token(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(is_token_byte)
}

fn has_type_grammar(value: &str) -> bool {
    value.len() <= MAX_PAYLOAD_TYPE_LEN
        && value
            .split_once('/')
            .is_some_and(|(kind, subtype)| is_token(kind) && is_token(subtype))
}

fn malformed(header: &str) -> CratestackError {
    CratestackError::BadRequest(format!("malformed {header} header"))
}

/// Parse a received [`PAYLOAD_TYPE_HEADER`] value, strictly.
///
/// Checks the grammar only; whether the type may be sealed or is allowed for
/// an op is the caller's decision ([`is_sealable_payload_type`], the op's
/// declared types).
///
/// ```
/// use cratestack_core::parse_payload_type;
///
/// assert_eq!(
///     parse_payload_type(b"application/x-www-form-urlencoded").unwrap(),
///     "application/x-www-form-urlencoded"
/// );
/// assert!(parse_payload_type(b"Application/JSON").is_err());
/// assert!(parse_payload_type(b"application/json; charset=utf-8").is_err());
/// ```
pub fn parse_payload_type(value: &[u8]) -> Result<&str, CratestackError> {
    std::str::from_utf8(value)
        .ok()
        .filter(|value| has_type_grammar(value))
        .ok_or_else(|| malformed(PAYLOAD_TYPE_HEADER))
}

/// Parse a received [`PAYLOAD_ACCEPT_HEADER`] value, strictly: the sender's
/// order is kept, entries are separated by exactly `", "`, and a repeated
/// entry is refused so the list has one spelling.
///
/// ```
/// use cratestack_core::parse_payload_accept;
///
/// assert_eq!(
///     parse_payload_accept(b"application/json, application/cbor").unwrap(),
///     ["application/json", "application/cbor"]
/// );
/// assert!(parse_payload_accept(b"application/json,application/cbor").is_err());
/// assert!(parse_payload_accept(b"application/json;q=0.5").is_err());
/// ```
pub fn parse_payload_accept(value: &[u8]) -> Result<Vec<&str>, CratestackError> {
    let text = std::str::from_utf8(value).map_err(|_| malformed(PAYLOAD_ACCEPT_HEADER))?;
    let entries: Vec<&str> = text.split(", ").collect();
    let well_formed = entries.len() <= MAX_PAYLOAD_ACCEPT_ENTRIES
        && entries.iter().all(|entry| has_type_grammar(entry))
        && entries
            .iter()
            .enumerate()
            .all(|(index, entry)| !entries[..index].contains(entry));
    if well_formed {
        Ok(entries)
    } else {
        Err(malformed(PAYLOAD_ACCEPT_HEADER))
    }
}

/// The [`PAYLOAD_ACCEPT_HEADER`] value for `types`, the way
/// [`parse_payload_accept`] reads it back.
///
/// ```
/// use cratestack_core::payload_accept_header_value;
///
/// assert_eq!(
///     payload_accept_header_value(&["application/json", "application/cbor"]),
///     "application/json, application/cbor"
/// );
/// ```
pub fn payload_accept_header_value(types: &[&str]) -> String {
    types.join(", ")
}

/// Whether `media_type` may be the type of a sealed payload: the grammar
/// above, and not an envelope in an envelope (`application/cose*`), a
/// stream that cannot be sealed until ADR 0006 P1 (`application/cbor-seq`,
/// `text/event-stream`) or a multipart body.
///
/// ```
/// use cratestack_core::is_sealable_payload_type;
///
/// assert!(is_sealable_payload_type("application/json"));
/// assert!(!is_sealable_payload_type("application/cose"));
/// assert!(!is_sealable_payload_type("text/event-stream"));
/// assert!(!is_sealable_payload_type("application/json; charset=utf-8"));
/// ```
pub fn is_sealable_payload_type(media_type: &str) -> bool {
    has_type_grammar(media_type)
        && !media_type.starts_with("application/cose")
        && !matches!(media_type, "application/cbor-seq" | "text/event-stream")
        && !media_type.starts_with("multipart/")
}
