# @cratestack/cbor-node

Native N-API CBOR codec for CrateStack's generated TypeScript RPC clients — a
[`CratestackRpcCodec`](https://github.com/cratestack/cratestack/blob/main/packages/cratestack-ts-types/src/index.ts)
backed by the framework's own [`cratestack-codec-cbor`](https://github.com/cratestack/cratestack/tree/main/crates/cratestack-codec-cbor)
Rust crate via native bindings ([`crates/cratestack-cbor-napi`](https://github.com/cratestack/cratestack/tree/main/crates/cratestack-cbor-napi)),
not a JS reimplementation. Encode/decode output is byte-identical to what the
CrateStack server and Rust client already produce for `application/cbor`.

Ships prebuilt per-platform binaries via `optionalDependencies` (the same pattern
`esbuild`/`@swc/core` use) — no Rust toolchain needed to install or use this package.

See [epic #285](https://github.com/cratestack/cratestack/issues/285) and
[issue #286](https://github.com/cratestack/cratestack/issues/286) for the full design context.

## Usage

```ts
import { cborCodec } from "@cratestack/cbor-node";

const bytes = cborCodec.encode({ hello: "world", count: 1 });
const value = cborCodec.decode(bytes);
```

`cborCodec` structurally satisfies `CratestackRpcCodec` from `@cratestack/ts-types`, so it
can be passed anywhere a generated client's runtime expects a codec — no explicit
dependency on `@cratestack/ts-types` required at runtime, since it's checked via
`satisfies`, not a type re-export.

## Null handling

CBOR has a real `null` (RFC 8949 §3.3, simple value 22, byte `0xf6`). `cratestack-codec-cbor`'s
underlying `minicbor-serde` backend has a documented quirk where a bare Rust unit type `()`
encodes as the CBOR *empty array* marker (`0x80`) instead — this package's `encode` translates
every JSON `null` (top-level or nested inside an object/array) through the code path that
produces real CBOR null, matching `CborCodec`'s own `Option::None` behavior byte-for-byte. See
`crates/cratestack-cbor-napi/src/json_value.rs` for the detail.

## Error handling

Malformed CBOR bytes passed to `decode` throw a catchable JS `Error` — never a native crash.
Both native entry points also use napi-rs's `catch_unwind` to convert any unexpected Rust panic
into a catchable exception instead of aborting the Node process.

## Scope

Single-item encode/decode only — no CBOR-seq/streaming support (that's a separate concern; see
epic #285). Since issue #746, `@cratestack/cbor` (this package's Node half, via the umbrella
`@cratestack/cbor` re-export) **is** the default codec for a generated TypeScript RPC client —
`--no-native-cbor` (`TypeScriptGeneratorConfig::native_cbor: false`) falls back to the
pure-TypeScript `jsonRpcCodec`, needed on platforms this package doesn't ship a napi binary for
(musl/Alpine Linux and `win32-arm64` — see `napi.targets` in `package.json`). REST-transport
generated clients are unaffected either way: the REST runtime has no codec seam at all.
