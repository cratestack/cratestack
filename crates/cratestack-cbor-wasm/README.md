# cratestack-cbor-wasm

`wasm-bindgen` bindings exposing `cratestack-codec-cbor`'s `CborCodec` to
browser JavaScript, built into the `@cratestack/cbor-web` npm package
([`packages/cratestack-cbor-web`](../../packages/cratestack-cbor-web)).

## What this is (and isn't)

This crate contains no CBOR encode/decode logic of its own — it wraps the
existing, already-tested `CborCodec` from `cratestack-codec-cbor` for
`wasm32-unknown-unknown`. It ships no useful Rust API (`crate-type =
["cdylib"]`, no `rlib`) and is not published to crates.io (`publish =
false`); its only consumer is `wasm-pack`, and its only artifact is the
`.wasm` + JS glue that `@cratestack/cbor-web` re-exports.

## Building

```sh
rustup target add wasm32-unknown-unknown  # once
wasm-pack build --target web crates/cratestack-cbor-wasm
```

Everyday `cargo check --workspace` / `cargo test --workspace` do **not**
need the wasm32 target: every `wasm-bindgen`-specific item (and its
dependencies) is gated behind `cfg(target_arch = "wasm32")`, so on a plain
host toolchain this crate compiles to effectively nothing. The
null-encoding correctness tests in `src/json_bridge.rs` are plain Rust and
run on the host via `cargo test -p cratestack-cbor-wasm`; the
`wasm_bindgen_test` suite in `src/wasm.rs` needs the wasm32 target and
runs via `wasm-pack test --node crates/cratestack-cbor-wasm` (with `--features cose` the suite is configured for the browser: `wasm-pack test --headless --chrome -- --features cose`).

## Public JS surface

- `contentType(): string` — `"application/cbor"`.
- `encode(value: unknown): Uint8Array` — throws (catchable `Error`, never
  a wasm trap) on values that can't be represented.
- `decode(bytes: Uint8Array): unknown` — throws (catchable `Error`, never
  a wasm trap) on malformed CBOR input.

With the **`cose` feature** (off by default; only the `cratestack_cbor` Dart package's web artifact is
built with it, `just cbor-vendor-web`; `@cratestack/cbor-web` is not, and CI asserts it) the module also
exports the COSE signed transport of ADR 0006 (cratestack#1026), over `cratestack-cose`:

- `ClientEnvelope.hmac(alg, secret, serverKeys, audience, options?)` and
  `ClientEnvelope.ed25519Seed(seed, serverKeys, audience, options?)` — `alg` is `"hmac256-64"` or
  `"hmac256-256"`, `serverKeys` is `[{ alg, bytes }]`, `options` (tests only) is `{ fixedIat?, fixedCti? }`.
  They throw `{ code, message }`.
- `envelope.sealRequest(payload, binding): Promise<Uint8Array>` and
  `envelope.openResponse(body, binding, sealedRequest, status): Promise<{ payload, kid, alg, thumbprint }>`,
  with `binding` = `{ method, route, pathParams, query?, contractSha, idempotencyKey?, ifMatch? }`.
  Rejections are `{ code, message }`: `"rejected"` (every failed verification, empty message) or `"misuse"`.
- `envelope.mode`, `envelope.mediaType`, `envelope.kid`, and `contractHeaderValue(contractSha): string`.

`packages/cratestack-cbor-web` wraps these into an async
`createCborCodec()` factory that performs the one-time wasm module
instantiation and returns a plain object satisfying
`CratestackRpcCodec` from `@cratestack/ts-types` — every `encode`/`decode`
call on the returned object is synchronous.

## See Also

- `crates/cratestack-codec-cbor` — the wrapped codec, unchanged.
- `crates/cratestack-rusqlite`, `examples/embedded-browser-vite` —
  the `wasm32-unknown-unknown` + `wasm-bindgen` conventions this crate
  follows.

## License

MIT
