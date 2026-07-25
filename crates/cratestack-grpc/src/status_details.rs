//! Structured error `details` on the wire — `docs/design/protobuf.md` §7.3:
//! `RpcErrorBody.details` rides in the `grpc-status-details-bin` metadata
//! key.
//!
//! **Judgment call, documented per ticket #171's ask:** the real
//! `google.rpc.Status` proto wraps `details` in `repeated google.protobuf.Any`,
//! which needs a type-URL registry and full `Any` (de)packing —
//! disproportionate machinery for CrateStack's one concrete use today (an
//! arbitrary `serde_json::Value` payload, e.g. validation issues) and a
//! second protobuf-ecosystem dependency (`tonic-types`, whose main value
//! here is exactly that `Any`-packing convenience) for a shape `prost`
//! alone — already a dependency — can encode directly. So this module
//! hand-rolls a **simplified** status-details message: `code` (1) +
//! `message` (2) + `details_json` (3, raw UTF-8 JSON bytes, not an `Any`).
//! Fields 1/2 use the same numbers `google.rpc.Status` itself assigns them,
//! so a consumer that only reads `code`/`message` via the real
//! `google.rpc.Status` proto still decodes them correctly; `details_json`
//! at tag 3 is a deliberate, documented deviation from
//! `google.rpc.Status.details` (tag 3, `repeated Any`) — a client that
//! expects genuine `Any` values there will not parse this field. Revisit if
//! a concrete consumer needs real `Any`-typed details.

use base64::Engine;
use prost::Message;

/// Wire-compatible with `google.rpc.Status`'s `code` (1) and `message` (2)
/// fields; `details_json` (3) is this crate's own simplified encoding —
/// see the module doc for why it isn't a real `google.protobuf.Any`.
#[derive(Clone, PartialEq, Message)]
pub struct GrpcStatusDetails {
    #[prost(int32, tag = "1")]
    pub code: i32,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(bytes = "vec", tag = "3")]
    pub details_json: Vec<u8>,
}

/// gRPC metadata values are ASCII; binary metadata keys (the `-bin` suffix,
/// `grpc-status-details-bin` among them) are base64-encoded by convention
/// across every gRPC implementation, tonic included. This function returns
/// the encoded string so callers can insert it under that key with
/// whichever metadata API they're using.
pub fn encode_status_details_bin(
    tonic_code: tonic::Code,
    message: &str,
    details: Option<&serde_json::Value>,
) -> String {
    let details_json = details
        .map(serde_json::Value::to_string)
        .unwrap_or_default();
    let payload = GrpcStatusDetails {
        code: tonic_code as i32,
        message: message.to_owned(),
        details_json: details_json.into_bytes(),
    };
    base64::engine::general_purpose::STANDARD.encode(payload.encode_to_vec())
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeStatusDetailsError {
    #[error("grpc-status-details-bin is not valid base64: {0}")]
    Base64(base64::DecodeError),
    #[error("grpc-status-details-bin did not decode as GrpcStatusDetails: {0}")]
    Proto(prost::DecodeError),
}

pub fn decode_status_details_bin(
    encoded: &str,
) -> Result<GrpcStatusDetails, DecodeStatusDetailsError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(DecodeStatusDetailsError::Base64)?;
    GrpcStatusDetails::decode(bytes.as_slice()).map_err(DecodeStatusDetailsError::Proto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_code_message_and_details() {
        let details = serde_json::json!({"field": "email", "issue": "invalid"});
        let encoded =
            encode_status_details_bin(tonic::Code::InvalidArgument, "bad input", Some(&details));
        let decoded = decode_status_details_bin(&encoded).unwrap();

        assert_eq!(decoded.code, tonic::Code::InvalidArgument as i32);
        assert_eq!(decoded.message, "bad input");
        let round_tripped: serde_json::Value =
            serde_json::from_slice(&decoded.details_json).unwrap();
        assert_eq!(round_tripped, details);
    }

    #[test]
    fn no_details_encodes_empty_json_bytes() {
        let encoded = encode_status_details_bin(tonic::Code::NotFound, "missing", None);
        let decoded = decode_status_details_bin(&encoded).unwrap();
        assert!(decoded.details_json.is_empty());
    }

    #[test]
    fn rejects_non_base64_input() {
        assert!(decode_status_details_bin("not base64!!").is_err());
    }
}
