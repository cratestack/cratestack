//! Media-type checks: which requests carry an envelope, and what `Accept`
//! asks for.

use http::header::{ACCEPT, CONTENT_TYPE};
use http::{HeaderMap, HeaderValue};

use super::PAYLOAD_MEDIA_TYPE;
use super::server_envelope::ServerEnvelope;

/// The base type every COSE framing shares (RFC 9052 §2); `cose-type` is a
/// parameter.
const COSE_BASE: &str = "application/cose";

/// Whether any `Content-Type` header names an envelope: the base type
/// `application/cose` in any case with any parameters, or a type `envelope`
/// claims.
///
/// Checked over **every** `Content-Type` value, and a value that is not
/// valid UTF-8 is compared byte-wise: a request must not escape the opener
/// by sending a second header, odd casing or a non-ASCII byte after the
/// type. It errs towards "enveloped", which is the side that fails closed.
pub(super) fn is_envelope_request(headers: &HeaderMap, envelope: &dyn ServerEnvelope) -> bool {
    headers.get_all(CONTENT_TYPE).iter().any(|value| {
        names_cose(value.as_bytes())
            || value
                .to_str()
                .is_ok_and(|value| envelope.is_envelope_content_type(value))
    })
}

/// Whether `media_type` (the envelope's own, or one a [`super::Sealed`]
/// names) is an envelope type: `application/cose`, or one the envelope
/// claims. What the builder checks and every sealed response must pass,
/// so a response is never labelled as something a client reads as plain.
pub(super) fn is_envelope_media_type(media_type: &str, envelope: &dyn ServerEnvelope) -> bool {
    names_cose(media_type.as_bytes()) || envelope.is_envelope_content_type(media_type)
}

fn names_cose(value: &[u8]) -> bool {
    let base = value.split(|byte| *byte == b';').next().unwrap_or(value);
    base.trim_ascii().eq_ignore_ascii_case(COSE_BASE.as_bytes())
}

/// Whether an `Accept` entry with a non-zero weight names an envelope.
pub(super) fn accept_names_envelope(headers: &HeaderMap, envelope: &dyn ServerEnvelope) -> bool {
    accept_entries(headers)
        .any(|entry| names_cose(entry.as_bytes()) || envelope.is_envelope_content_type(entry))
}

/// Whether an `Accept` entry with a non-zero weight names a streamed
/// representation (`application/cbor-seq` or `text/event-stream`).
pub(super) fn accept_names_stream(headers: &HeaderMap) -> bool {
    accept_entries(headers).any(|entry| {
        let base = entry.split(';').next().unwrap_or(entry).trim();
        base.eq_ignore_ascii_case(crate::CBOR_SEQUENCE_CONTENT_TYPE)
            || base.eq_ignore_ascii_case("text/event-stream")
    })
}

/// Every `Accept` entry, across every `Accept` header, minus those with
/// `q=0` (explicitly refused, RFC 9110 §12.5.1).
fn accept_entries(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !refused(entry))
}

fn refused(entry: &str) -> bool {
    entry.split(';').skip(1).any(|param| {
        let Some((name, value)) = param.split_once('=') else {
            return false;
        };
        name.trim().eq_ignore_ascii_case("q") && value.trim().parse::<f32>().is_ok_and(|q| q == 0.0)
    })
}

/// Whether a response's `Content-Type` is exactly the payload media type
/// the binding names (parameters aside), so its body can be sealed as is.
pub(super) fn is_cbor_response(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return false;
    };
    value.to_str().is_ok_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or(value)
            .trim()
            .eq_ignore_ascii_case(PAYLOAD_MEDIA_TYPE)
    })
}

pub(super) fn cbor_header_value() -> HeaderValue {
    HeaderValue::from_static(PAYLOAD_MEDIA_TYPE)
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue};

    use super::{accept_names_stream, is_cbor_response, names_cose, refused};

    #[test]
    fn cose_is_recognised_in_every_spelling() {
        for value in [
            "application/cose",
            "Application/COSE",
            " application/cose ; cose-type=\"cose-sign1\"",
            "application/cose;cose-type=cose-mac0",
        ] {
            assert!(names_cose(value.as_bytes()), "{value}");
        }
        assert!(
            names_cose(b"application/cose; x=\xff"),
            "non-UTF-8 parameters"
        );
        for value in [
            "application/cbor",
            "application/cose-key",
            "application/coses",
        ] {
            assert!(!names_cose(value.as_bytes()), "{value}");
        }
    }

    #[test]
    fn a_zero_weight_entry_is_not_asked_for() {
        assert!(refused("application/cbor-seq;q=0"));
        assert!(refused("application/cbor-seq; q=0.0"));
        assert!(!refused("application/cbor-seq;q=0.1"));
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::ACCEPT,
            HeaderValue::from_static("application/cbor-seq;q=0, application/cbor"),
        );
        assert!(!accept_names_stream(&headers));
    }

    #[test]
    fn a_response_with_two_content_types_is_not_cbor() {
        let mut headers = HeaderMap::new();
        headers.append(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/cbor"),
        );
        assert!(is_cbor_response(&headers));
        headers.append(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        assert!(!is_cbor_response(&headers));
    }
}
