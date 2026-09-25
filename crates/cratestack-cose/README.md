# cratestack-cose

**L2 — Adapters.** The COSE envelope of ADR 0006: a signed body is the CBOR codec's output
wrapped as a COSE_Sign1 or COSE_Mac0, bound to its request through external AAD that both
sides rebuild and nobody sends.

```text
typed value ──CborCodec──▶ payload bytes ──CoseEnvelope──▶ COSE_Sign1 / COSE_Mac0
```

This is P0: unary messages, `nonce` replay, and the shared test vectors. `chain` streams
(P1) and `window` replay for device keys (P2) come later.

Without features the crate depends on `cratestack-core` alone, and compiles for
`wasm32-unknown-unknown`. The off-by-default `auth` feature adds `cratestack-auth` (see
below).

## What is here

```rust,ignore
use cratestack_cose::{CoseEnvelope, CoseMode, Ed25519Signer, StaticVerifierResolver};

let server = CoseEnvelope::server(CoseMode::Sign1, signer, resolver, nonce_store).build()?;

// Through `cratestack_core::CratestackEnvelope` (records a `VerifiedSigner`, naming the
// verifying key's thumbprint, in the context):
let payload = server.open(body, &binding, &mut ctx).await?;

// Or typed, before any context exists (the axum layer, cratestack#1006):
let opened = server.open_request(body, &binding).await?; // payload, kid, alg, thumbprint, iat, cti

// Sealing a value encodes it straight into the message buffer:
let sealed = server.seal_response_value(&CborCodec, &row, &response_binding).await?;
```

- **Algorithms:** Ed25519 (`-19`, the default) and ESP256 (`-9`, low-`s` only) for Sign1;
  HMAC 256/64 (`4`) and 256/256 (`5`) for Mac0. Nothing else is accepted, including the
  deprecated `-8` and `-7`. A key verifies exactly one algorithm.
- **Header:** protected `{1: alg, 4: kid, ? 15: {6: iat, 7: cti}}` (claims on requests
  only), unprotected always empty. The `kid` is the first 8 bytes of the key's RFC 9679
  thumbprint (`cratestack_cose::thumbprint`), and a key verifies only under its own `kid`.
- **AAD:** `[1, audience, method, route, path_params, query / null, schema_sha,
  payload_type, ? request_kind, ? request_digest, ? status]`; see `external_aad`.
  `audience` is the receiving service's configured id. It must not be empty (a `500`), and a
  service's inbound audience must differ from the audience it seals its outbound requests
  for: a name shared by both directions, such as `internal`, gives up reflection
  protection. A response to a signed request is bound to `request_digest` (kind `1`,
  SHA-256 of the request's COSE bytes); a response to an unsigned one to
  `request_digest_unsigned` (kind `0`, SHA-256 of the client's `Cratestack-Nonce` and the
  payload; see `RequestNonce`). Both return the kind with the digest. Sending and reading
  that header is wired in cratestack#1006/#1007.
- **Errors:** every failed check is the same `401`; a failing key resolver, nonce store or
  signer, and local misuse, is a `500`.
- **Keys:** `CoseSigner` signs without exporting the key (KMS, HSM); `CoseVerifierResolver`
  returns every candidate for a `kid`; `CoseVerifyKey` is opaque and typed, so an Ed25519
  public key can never be used as an HMAC secret; `KeyProviderMacKeys` loads Mac0 keys from
  core's `KeyProvider`. HMAC secrets must be random: a Mac0 `kid` publishes 64 bits of the
  secret's thumbprint, so a guessable secret can be found offline.

## The `auth` feature

`cratestack_cose::auth` connects the envelope to `cratestack-auth` (the edge points cose ->
auth, never the reverse):

- `ServiceKeySigner`: a `ServiceSigningKey` as an Ed25519 `CoseSigner`. Its COSE `kid` is the
  thumbprint prefix of the key, **not** the key's human JWKS label (`ServiceSigningKey::kid`).
- `DeviceKeyCoseResolver`: a `DeviceKeyResolver` as a `CoseVerifierResolver`, through the
  resolver's required `lookup_device_verifying_keys_by_thumbprint`. Ed25519 only; an
  unknown device is the `401`, a failing registry the `500`.
- `AuthNonceStore`: `cratestack-auth`'s nonce store, in-memory or Redis
  (`AuthNonceStore::redis(url)`, which refuses an empty URL rather than falling back to
  memory), as core's `NonceStore`. Entries are `(kid, cti)` and live until at least
  `iat + 2·skew + 1`; Redis `SET NX EX` makes the check atomic across replicas.
- `build_cose_enroll_response` / `parse_cose_enroll_response`: the enrolment challenge,
  moved here unchanged from `cratestack-auth`. It keeps its legacy shape (alg `-8`, a
  35-byte `kid`, empty AAD) and its own `coset`-based code path; the strict opener above
  never accepts it.

## Shared vectors

`tests/vectors/*.json` hold the fixed keys, the 112-byte payment fixture, 33 unary cases
and 20 must-reject cases in hex, for the wasm, napi, TypeScript and Dart bindings to check
themselves against. Each case carries its AAD, protected header and to-be-signed bytes.
The Ed25519 and HMAC cases are byte-exact (`deterministic: true`); ESP256 cases were made
with RFC 6979 and low-`s`, and another implementation should verify them rather than
reproduce them. **An ESP256 sender MUST emit low-`s`**: a verifier refuses a high `s`
(`neg-esp256-high-s`), so an implementation whose signer may return either (WebCrypto, a
KMS) normalises before sending. Every positive case names the key that verifies it
(`key`, an entry of `keys.json`, each of which carries its algorithm) and, for requests,
the verifier's clock and skew. The keys in them are published test keys; never use them
for anything else.
