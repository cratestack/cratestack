# COSE signed mode for the TypeScript clients

Status: **implemented in part** (cratestack#1010): the Rust bindings, the package exports and the
sealing link/fetch for the generated REST and RPC runtimes. Section 7 lists what is not. ADR 0006 (`internals/cose-envelope-adr.md` in cratestack-docs) owns the
envelope; this note owns how TypeScript reaches it.

## 1. The rule

All crypto, hashing, CBOR framing, the canonical query and the AAD stay in `cratestack-cose`.
TypeScript carries bytes to it and back, builds headers, and calls the caller's signing callback
with the to-be-signed bytes the Rust side hands it. It never computes a signature, a digest or a
canonical form.

## 2. What existed

`cratestack-cbor-wasm` already had a `cose` feature and a `ClientEnvelope` class (HMAC and
Ed25519-seed signers, `sealRequest`, `openResponse`, `selectorHeaders`) for the Dart web backend
(cratestack#1026). `@cratestack/cbor-web` was built without it and CI asserted that
(`.ci/cbor-web-codec-only.sh`). `cratestack-cbor-napi` had no COSE at all. Nothing implements
`StreamOpener` in Rust yet (P1, cratestack#1008), and `cratestack-cose` has no founder/intermediate
trust chain; both are out of scope here (section 7).

## 3. Decisions

1. **A second wasm artifact, not a bigger one.** `@cratestack/cbor-web` keeps `dist/wasm-pkg`
   byte for byte as it was. A second build of the same crate with `--features cose` goes to
   `dist/wasm-pkg-cose` behind the `./cose` subpath. A client that does not sign downloads
   nothing new, so the codec bundle budget is unchanged; a signing client loads both modules.
   The codec-only guard keeps its meaning and gains the converse check (the cose artifact has the class).
2. **napi: one addon, no new targets.** `cratestack-cbor-napi` depends on `cratestack-cose`
   (already pure Rust, no new crates in the lockfile beyond the two features of `bytes` and
   `zeroize`). The prebuilt target list is untouched; the `.node` grows by the measured amount in
   the PR description.
3. **External signer.** wasm: `ClientEnvelope.external(alg, publicKey, sign, ...)` where `sign`
   is `(toBeSigned) => Promise<Uint8Array>`; it is `cratestack_cose::ExternalSigner` with a JS
   function as the callback, so DER to raw `r || s`, ESP256 low-`s` and "a signature that does not
   verify under the public key is refused" are Rust's. napi has no async runtime (and gets none:
   no `tokio` in the addon), so sealing is two steps: `beginSealRequest` runs the seal up to the
   signer and exposes `toBeSigned`; `finish(signature)` resumes it. The package's TypeScript
   wrapper awaits the callback between the two, so both packages present the same `CoseClient`.
4. **One TypeScript shape** (`@cratestack/ts-types`, `CoseClient`, types only plus `CoseFailure`):
   `createCoseClient({ signer, serverKeys, audience })` with `signer` one of `hmac` (Mac0 under a
   session key), `ed25519-seed`, `p256-scalar`, `external`. `sealRequest` /
   `openResponse` / `selectorHeaders` / `contractHeader` / `canonicalQuery`.
5. **Errors** are `CoseFailure` with `code` `rejected` (every failed verification, empty message)
   or `misuse`, the same as the web build's `{ code, message }`. A throwing or rejecting signer is
   `misuse` with a fixed message, never the page's error text.
6. **Vectors.** `crates/cratestack-cose/tests/vectors/{keys,unary}.json` is the one source. Rust
   native (`cratestack-cose`), wasm (`wasm-bindgen-test` in Chrome), napi (`cargo test`, the pure
   half) and the two JS packages (vitest, through the built artifacts) all seal the same 17
   requests to the same bytes and open the same 12 responses. A shared TypeScript suite
   (`@cratestack/ts-types/cose-conformance`) is what both packages run.

## 4. Mode per call

A `CoseClient` is Sign1 or Mac0 by its signer. Sign1 for the calls before a session exists,
Mac0 under the session key after it, no seal for anonymous calls: the link takes
`cose: CoseClient | ((route) => CoseClient | undefined)`, and `undefined` sends the call as it
would go without the link. Replacing the session key is a new client; there is no shared mutable
key state to race.

## 5. The link

Sealing wraps the **fetch** a call is about to make, because that is where the exact wire bytes
(after the codec and after compact keys) and the exact headers (`Idempotency-Key`, `If-Match`,
`Cratestack-Contract`, the compact `Content-Type` and `Accept`) exist. It binds what the request
carries, as the Rust client's `request_sealed` does. It lives in `@cratestack/cbor/cose`
(`packages/cratestack-cbor/src/sealed.ts`), not in generated code: a generated client composes it
and gains no new file, so no generated tree changes for it.

- **RPC.** `createSealedLink({ cose, contracts: OP_CONTRACTS })` is an `RpcLink` that replaces
  `request.fetchFn`; `route` is the op id (`batch` for `/rpc/batch`, which must be CBOR both
  ways), `contractSha` is `OP_CONTRACTS[route]`.
- **REST.** `createSealedFetch({ cose, contracts: OP_CONTRACTS, basePath })` is the `fetch`
  option of `CratestackRuntime`: the route template and path parameters come from matching the URL
  path against the `"<METHOD> <template>"` keys of `OP_CONTRACTS` (most literal template first); the
  query is canonicalised by Rust (`canonicalQuery`).
- **Headers.** `Content-Type` and `Accept` become the envelope's media type; the request's
  type and accept list become `Cratestack-Payload-Type` / `-Accept` (bound), through
  `selectorHeaders`; `Cratestack-Contract` is `contractHeader(OP_CONTRACTS[route])`.
- **Mode per call.** `cose` is a `CoseClient`, or a function of `{ route, method }` returning one
  or `undefined`: Sign1 before a session, Mac0 after, unsealed for anonymous calls.
- **Replies.** A COSE reply is opened and returned as a `Response` whose body is the payload and
  whose `Content-Type` is the type it was sealed under; one that does not open throws
  `CoseFailure` `rejected`. An unsigned reply to a refused call (`!ok`) is returned untouched, a
  hint and never proof; an unsigned *success* throws. `stream()` is not sealed.
- **415.** The generated compact layers gain `noteFailure(status)`: a `415` to a request that went
  out compact takes the op (and its `Idempotency-Key` pin) back to named bodies, as the Rust
  client's `note_failure` does. The runtimes call it on any non-ok answer, sealed or not, since a
  refusal is unsigned either way. The refused call is not retried.

## 6. Size and targets

The codec artifact (`dist/wasm-pkg`) is built exactly as before. `dist/wasm-pkg-cose` is the same
crate with `--features cose`: 561,214 bytes (234,452 gzipped) after `wasm-opt -O`, against 225,953
(89,053 gzipped) for the codec alone. `napi.targets` is unchanged (`just verify-napi-targets`).

## 7. Not in this change

- `StreamOpener` (chunk-fed `chain` verification): `cratestack-cose` does not implement
  `StreamSealer`/`StreamOpener` yet (P1, cratestack#1008, #1009). Binding a trait nothing
  implements would ship an API that always refuses.
- Trust chain (founder list, intermediates, replica certificate in the reply): no such verifier
  exists in `cratestack-cose`; `serverKeys` is the static trust root the Rust client has.
- React Native (epic #893).
