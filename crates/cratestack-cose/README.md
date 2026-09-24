# cratestack-cose

**L2 — Adapters.** The COSE envelope of ADR 0006: a signed body is the CBOR codec's output
wrapped as a COSE_Sign1 or COSE_Mac0, bound to its request through external AAD that both
sides rebuild and nobody sends.

```text
typed value ──CborCodec──▶ payload bytes ──CoseEnvelope──▶ COSE_Sign1 / COSE_Mac0
```

This is P0: unary messages, `nonce` replay, and the shared test vectors. `chain` streams
(P1) and `window` replay for device keys (P2) come later. The `auth` feature, with the
`cratestack-auth` adapters, the Redis nonce bridge and the enrolment code, is the second
half of cratestack#1005 and is not here yet.

## What is here

```rust,ignore
use cratestack_cose::{CoseEnvelope, CoseMode, Ed25519Signer, StaticVerifierResolver};

let server = CoseEnvelope::server(CoseMode::Sign1, signer, resolver, nonce_store).build()?;

// Through `cratestack_core::CratestackEnvelope` (records a `VerifiedSigner` in the context):
let payload = server.open(body, &binding, &mut ctx).await?;

// Or typed, before any context exists (the axum layer, cratestack#1006):
let opened = server.open_request(body, &binding).await?; // payload, kid, alg, key_thumbprint, iat, cti
```

- **Algorithms:** Ed25519 (`-19`, the default) and ESP256 (`-9`) for Sign1; HMAC 256/64
  (`4`) and 256/256 (`5`) for Mac0. Nothing else is accepted, including the deprecated
  `-8` and `-7`.
- **Header:** protected `{1: alg, 4: kid, ? 15: {6: iat, 7: cti}}` (claims on requests
  only), unprotected always empty. The `kid` is the first 8 bytes of the key's RFC 9679
  thumbprint (`cratestack_cose::thumbprint`).
- **AAD:** `[1, method, route, path_params, query / null, schema_sha, payload_type,
  ? request_digest, ? status]`; see `external_aad` and `request_digest`.
- **Errors:** every failed check is the same `401`; a failing key resolver or nonce store
  is a `500`.
- **Keys:** `CoseSigner` signs without exporting the key (KMS, HSM); `CoseVerifierResolver`
  returns every candidate for a `kid`; `CoseVerifyKey` is typed, so an Ed25519 public key
  can never be used as an HMAC secret.

## Shared vectors

`tests/vectors/*.json` hold the fixed keys, the 112-byte payment fixture and 24 unary
cases in hex, for the wasm, napi, TypeScript and Dart bindings to check themselves
against. The Ed25519 and in-process ESP256 (RFC 6979) cases are byte-exact. The keys in
them are published test keys; never use them for anything else.
