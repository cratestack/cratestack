# @cratestack/cbor-web

A `wasm-bindgen` build of `cratestack-codec-cbor`'s `CborCodec` for browser
CrateStack RPC clients — the browser half of `@cratestack/cbor-node`'s
epic (#285). Same Rust encode/decode logic the server and Rust client
already use, reachable from browser JavaScript with no CBOR reimplemented
in TypeScript.

## Installation

```sh
npm install @cratestack/cbor-web
```

## Usage

`createCborCodec()` performs the one-time WASM module instantiation and
resolves to a plain object satisfying `CratestackRpcCodec` from
`@cratestack/ts-types`. Every `encode`/`decode` call on the returned
object is synchronous — the async cost is paid once, not per call:

```ts
import { createCborCodec } from "@cratestack/cbor-web";

const codec = await createCborCodec();

const bytes = codec.encode({ hello: "world" });
const value = codec.decode(bytes);
```

### With a generated CrateStack client

```ts
import { createCborCodec } from "@cratestack/cbor-web";
import { createClient } from "./generated/client.js";

const client = createClient({
  baseUrl: "https://api.example.com",
  codec: await createCborCodec(),
});
```

## Error handling

Malformed CBOR input on `decode`, or a value `encode` can't represent,
throws a catchable JS `Error` — never a WASM trap that would poison the
module for subsequent calls (see `crates/cratestack-cbor-wasm/src/wasm.rs`
for why that distinction matters).

## Bundlers

Ships a `wasm-pack --target web` build: the `.wasm` asset is resolved via
`new URL('cratestack_cbor_wasm_bg.wasm', import.meta.url)`, which Vite,
webpack 5, and Next.js all handle as a static asset without extra config.
Verified against `examples/embedded-browser-vite`.

## See Also

- `@cratestack/cbor-node` — the Node/native counterpart (napi-rs).
- `crates/cratestack-cbor-wasm` — the wasm-bindgen crate this package wraps.
- `crates/cratestack-codec-cbor` — the underlying, unchanged Rust codec.

## License

MIT
