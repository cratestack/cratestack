//! Envelope canonicalization for gRPC calls — `docs/design/protobuf.md`
//! §7.3. Builds on `cratestack_core::canonical_request_string`, the same
//! function every REST/RPC client-side request authorizer already calls
//! (`cratestack-client-rust::client::headers::build_header_map`); a gRPC
//! call reduces to the same four inputs (method/path/query/body) with
//! gRPC-specific values plugged in, so both seal and verify — whichever
//! `AuthProvider` a schema configures — agree on one canonical string
//! across every transport CrateStack speaks.
//!
//! There is no framework-level "response signing" step wired into the RPC
//! binding today: `canonical_request_string` is a canonicalization utility
//! an `AuthProvider` implementation calls when it wants signature
//! verification, not something `cratestack-axum`'s dispatch functions
//! invoke automatically (confirmed by grepping `cratestack-axum` for the
//! function: no call sites — only the client-side request authorizer and
//! `cratestack-core` itself reference it). This module's job is therefore
//! narrower than "seal/verify a gRPC envelope": it gives a custom
//! `AuthProvider` written for a `transport grpc` schema the same canonical
//! string a REST/RPC `AuthProvider` would compute, from gRPC-native inputs,
//! so hand-written signing logic doesn't have to re-derive it per schema.
//!
//! `framed_body` here is always plain (non-gRPC-Web) framing, even when
//! the call arrived over gRPC-Web — see [`crate::framing`]'s module doc
//! for why `tonic_web::GrpcWebLayer` guarantees that.

use cratestack_core::canonical_request_string;

use crate::framing::{FrameError, strip_grpc_frame};

/// gRPC always pins `application/grpc+proto` as the negotiated content type
/// for a CrateStack service (`docs/design/protobuf.md` §7.3: content
/// negotiation does not exist on this binding). This is the value plugged
/// into `canonical_request_string`'s `content_type` slot.
pub const GRPC_CONTENT_TYPE: &str = "application/grpc+proto";

/// The gRPC wire path for a method: `/<package>.Api/<MethodName>` —
/// `docs/design/protobuf.md` §4.6's flat single-service layout. `package`
/// is the schema's locked `.pb.lock` package name; `method_name` is the
/// PascalCase, dot-dropped op id (`cratestack_proto::casing::op_id_to_method_name`).
pub fn grpc_method_path(package: &str, method_name: &str) -> String {
    format!("/{package}.Api/{method_name}")
}

/// Builds the canonical signing string for a gRPC call. `query` is always
/// `None` — gRPC has no query string. `framed_body` is the raw wire bytes
/// as tonic's codec layer sees them (5-byte length-prefix frame still
/// attached); this function strips it before canonicalizing, so callers
/// don't have to remember to do that at every call site.
///
/// Returns an error if `framed_body` isn't validly framed. That should
/// never happen on a real request — tonic itself rejects a malformed frame
/// during decode, before a handler (and therefore this function) ever sees
/// it — so this is a defensive check, not an expected runtime path.
pub fn grpc_canonical_request_string(
    package: &str,
    method_name: &str,
    framed_body: &[u8],
) -> Result<String, FrameError> {
    let path = grpc_method_path(package, method_name);
    let body = strip_grpc_frame(framed_body)?;
    Ok(canonical_request_string(
        "POST",
        &path,
        None,
        Some(GRPC_CONTENT_TYPE),
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::frame_grpc_message;

    #[test]
    fn method_path_matches_design_doc_worked_example() {
        assert_eq!(
            grpc_method_path("shop_api", "ModelUserList"),
            "/shop_api.Api/ModelUserList"
        );
    }

    #[test]
    fn canonical_string_matches_direct_core_call_on_unframed_bytes() {
        let message: &[u8] = &[0x08, 0x01]; // arbitrary prost-encoded bytes
        let framed = frame_grpc_message(message, false);

        let via_grpc = grpc_canonical_request_string("shop_api", "ModelUserGet", &framed).unwrap();
        let via_core = canonical_request_string(
            "POST",
            "/shop_api.Api/ModelUserGet",
            None,
            Some(GRPC_CONTENT_TYPE),
            message,
        );

        assert_eq!(via_grpc, via_core);
    }

    #[test]
    fn malformed_frame_is_rejected_rather_than_silently_signed() {
        let result = grpc_canonical_request_string("pkg", "Method", &[0, 0]);
        assert!(result.is_err());
    }
}
