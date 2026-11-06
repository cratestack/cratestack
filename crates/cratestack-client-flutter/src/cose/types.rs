//! The value types of the COSE bridge. Their mapping to `cratestack-cose`'s
//! is in `convert.rs`.

/// The four algorithms of ADR 0006 §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterCoseAlg {
    /// Ed25519 (`-19`), Sign1.
    Ed25519,
    /// ESP256 (`-9`), Sign1, low-`s` only.
    Esp256,
    /// HMAC 256/64 (`4`), Mac0.
    Hmac256_64,
    /// HMAC 256/256 (`5`), Mac0.
    Hmac256_256,
}

/// The message structure an envelope emits and accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterCoseMode {
    /// COSE_Sign1 (tag 18), `application/cose; cose-type="cose-sign1"`.
    Sign1,
    /// COSE_Mac0 (tag 17), `application/cose; cose-type="cose-mac0"`.
    Mac0,
}

/// A key the server's responses verify with, pinned at enrolment.
///
/// `Debug` shows the algorithm and the length of `bytes`, never `bytes`:
/// for a COSE_Mac0 server it holds the shared secret.
#[derive(Clone, PartialEq, Eq)]
pub struct FlutterServerKey {
    /// The algorithm this key verifies, and only it.
    pub alg: FlutterCoseAlg,
    /// Ed25519: the 32-byte public key. ESP256: the SEC1 point
    /// (compressed or uncompressed). HMAC: the shared secret.
    pub bytes: Vec<u8>,
}

/// The inputs of one call's binding. The audience is the envelope's, not
/// the call's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterCallBinding {
    /// HTTP method, e.g. `"POST"`.
    pub method: String,
    /// The RPC `op_id`, or the REST route template.
    pub route: String,
    /// REST path parameter values in template order; empty for RPC.
    pub path_params: Vec<String>,
    /// The query string in any spelling; it is canonicalised.
    pub query: Option<String>,
    /// The op contract digest, 32 bytes. A `Vec` and not `[u8; 32]`:
    /// `flutter_rust_bridge` renders an array as a generated class that
    /// needs `package:collection`, which this package does not depend on.
    pub contract_sha: Vec<u8>,
    /// The `Idempotency-Key` the request will carry, if any.
    pub idempotency_key: Option<String>,
    /// The `If-Match` the request will carry, if any.
    pub if_match: Option<String>,
    /// The type the request payload is sealed under (bound in the AAD);
    /// `None` is `application/cbor`. Compact wire keys:
    /// `application/vnd.cratestack.compact+cbor`.
    pub payload_type: Option<String>,
    /// The types the answer may come back in, in order of preference;
    /// `None` is just `application/cbor`.
    pub payload_accept: Option<Vec<String>>,
}

/// One header the caller must send with a sealed call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterSelectorHeader {
    /// `Cratestack-Payload-Type` or `Cratestack-Payload-Accept`.
    pub name: String,
    /// Its value.
    pub value: String,
}

/// A response that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterOpened {
    /// The payload, exactly as signed.
    pub payload: Vec<u8>,
    /// The type the response was sealed under: one of the call's
    /// `payload_accept` (`application/cbor` by default).
    pub payload_type: String,
    /// The signer's 8-byte `kid`.
    pub kid: Vec<u8>,
    /// The algorithm it verified with.
    pub alg: FlutterCoseAlg,
    /// The RFC 9679 thumbprint of the key that verified (32 bytes).
    pub thumbprint: Vec<u8>,
}

/// Pins `iat` and `cti` so the shared vectors reproduce. **For tests
/// only**: a fixed `cti` is a replayed request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlutterSealOptions {
    /// `iat` in Unix seconds, instead of the clock.
    pub fixed_iat: Option<i64>,
    /// `cti` (1 to 4 or 16 bytes), instead of random bytes.
    pub fixed_cti: Option<Vec<u8>>,
}

impl std::fmt::Debug for FlutterServerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlutterServerKey")
            .field("alg", &self.alg)
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .finish()
    }
}
