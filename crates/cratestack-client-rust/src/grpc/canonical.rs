//! gRPC envelope-signing canonicalization for the native Rust client —
//! mirrors `cratestack_grpc::canonical::grpc_canonical_request_string`
//! (`docs/design/protobuf.md` §7.3), adapted for the client side of the
//! wire. This is the ticket's highest-risk decision (see #209's own risk
//! note); the reasoning below is the answer, not an assumption.
//!
//! **Why this does not reuse `cratestack_grpc::canonical` directly, and
//! why that's still correct:**
//!
//! `cratestack_grpc::canonical::grpc_canonical_request_string` canonicalizes
//! *received* wire bytes: it accepts the already length-prefix-*framed*
//! body exactly as `tonic::server::Grpc::unary` hands it to a service impl
//! (see that crate's `framing.rs` module doc), and strips the 5-byte frame
//! header before hashing — because on the server side, framed bytes are
//! all that's available to intercept (`tonic`'s own `Codec`/`Decoder`
//! chain decodes the message before any CrateStack code sees it; there is
//! no lower-level hook to grab raw bytes from — see that module's "Known
//! gap" note).
//!
//! On the **client** side there is no equivalent framed-bytes interception
//! point to begin with, and none is needed: this crate builds the request
//! message, canonicalizes it, *then* hands it to
//! `tonic::client::Grpc::unary`, which does the framing internally on the
//! way out. Signing happens strictly before framing exists. So the
//! correct client-side input is `prost::Message::encode_to_vec()`'s
//! output directly — which is *already* the same unframed bytes
//! `strip_grpc_frame` would produce if it ran on this crate's output after
//! tonic re-framed it. In other words: both sides converge on hashing the
//! identical byte sequence (the prost-encoded message, no frame), just
//! reached by different means — the server strips a frame down to it, the
//! client never adds one in the first place. `grpc_canonical_request_string`'s
//! own test (`canonical_string_matches_direct_core_call_on_unframed_bytes`)
//! already proves the server's stripped result equals calling
//! `cratestack_core::canonical_request_string` on the raw message bytes
//! directly — this module's `grpc_canonical_request_string` **is** that
//! direct call, so the two are provably byte-identical for the same
//! (package, method, message) triple without needing to share code.
//!
//! This also means the ticket's flagged risk — "don't assume this is a
//! drop-in reuse of the TS gRPC-Web client's base64/trailer-frame-specific
//! logic" — does not apply here at all: the TS client's constraints
//! (`grpc-web-text+proto`'s base64 encoding, gRPC-Web's response trailer
//! frame) are wire-transport peculiarities of gRPC-Web specifically, and a
//! native `tonic` client speaks plain binary gRPC framing, which (per
//! `cratestack_grpc::framing`'s own module doc) is exactly what a
//! `tonic::server::Grpc`-based CrateStack server always sees regardless of
//! which client dialect originated the call. Nothing gRPC-Web-specific
//! ever enters this module.
//!
//! **Why this is a small reimplementation of two constants rather than a
//! dependency on `cratestack-grpc`:** that crate is the *server* runtime —
//! it unconditionally depends on `axum`/`tonic-web`/`tower-http` (CORS,
//! gRPC-Web translation), none of which a pure gRPC client binary needs.
//! Pulling it in just to reach `GRPC_CONTENT_TYPE` and `grpc_method_path`
//! (two lines each) would tax every gRPC client build for zero benefit —
//! the same "small pure mapping gets reimplemented per crate/module"
//! precedent already established multiple times in this codebase (see
//! `cratestack-client-typescript::grpc::wire`'s module doc and
//! `cratestack-macros::include::client::grpc::rpc_inputs`'s module doc).
//! The cross-crate test below pins the two copies together so a future
//! change to either format doesn't drift silently.

use cratestack_core::canonical_request_string;

/// Pinned negotiated content type for every CrateStack gRPC call — same
/// value `cratestack_grpc::canonical::GRPC_CONTENT_TYPE` uses server-side
/// (`docs/design/protobuf.md` §7.3: no content negotiation on this
/// binding).
pub const GRPC_CONTENT_TYPE: &str = "application/grpc+proto";

/// `/<package>.Api/<MethodName>` — same shape as
/// `cratestack_grpc::canonical::grpc_method_path`.
pub fn grpc_method_path(package: &str, method_name: &str) -> String {
    format!("/{package}.Api/{method_name}")
}

/// Builds the canonical signing string for an outgoing gRPC call from the
/// **unframed** prost-encoded request bytes. `query` is always `None` —
/// gRPC has no query string — matching
/// `cratestack_grpc::canonical::grpc_canonical_request_string`'s output
/// exactly for the same (package, method, message) triple. See the module
/// doc for why no frame-stripping step belongs here.
pub fn grpc_canonical_request_string(
    package: &str,
    method_name: &str,
    message_bytes: &[u8],
) -> String {
    let path = grpc_method_path(package, method_name);
    canonical_request_string("POST", &path, None, Some(GRPC_CONTENT_TYPE), message_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_path_matches_server_side_shape() {
        assert_eq!(
            grpc_method_path("shop_api", "ModelUserList"),
            "/shop_api.Api/ModelUserList"
        );
    }

    /// Cross-crate parity: this must byte-for-byte match
    /// `cratestack_grpc::canonical::grpc_canonical_request_string` on the
    /// same (package, method, unframed-message) triple — one seals, the
    /// other verifies. `cratestack-grpc` isn't a dependency of this crate
    /// (see module doc), so this pins the expected value by calling
    /// `cratestack_core::canonical_request_string` directly with the same
    /// inputs `cratestack_grpc`'s own equivalent test uses
    /// (`canonical_string_matches_direct_core_call_on_unframed_bytes` in
    /// `crates/cratestack-grpc/src/canonical.rs`) — both tests must be
    /// updated in lockstep if either format ever changes.
    #[test]
    fn matches_cratestack_grpc_canonical_request_string_on_the_same_bytes() {
        let message: &[u8] = &[0x08, 0x01]; // arbitrary prost-encoded bytes
        let got = grpc_canonical_request_string("shop_api", "ModelUserGet", message);
        let expected = cratestack_core::canonical_request_string(
            "POST",
            "/shop_api.Api/ModelUserGet",
            None,
            Some("application/grpc+proto"),
            message,
        );
        assert_eq!(got, expected);
    }
}
