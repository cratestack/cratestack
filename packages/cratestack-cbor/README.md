# @cratestack/cbor

A thin umbrella package that auto-selects the right CBOR backend for
CrateStack's generated TypeScript RPC clients: `@cratestack/cbor-node`
(#286, native N-API) in Node, `@cratestack/cbor-web` (#287, WASM) in the
browser — one `npm install`, one import path, the environment picks the
implementation. The final piece of [epic #285](https://github.com/cratestack/cratestack/issues/285).

```sh
npm install @cratestack/cbor
```

```ts
import { createCborCodec } from "@cratestack/cbor";

const codec = await createCborCodec();

const bytes = codec.encode({ hello: "world" });
const value = codec.decode(bytes);
```

Node projects never download the WASM binary; browser bundles never pull
in native `.node` files — each platform's `dependencies` (`@cratestack/cbor-node`
or `@cratestack/cbor-web`) is only actually loaded once the matching
`exports` condition resolves.

## How auto-selection works

This package's `package.json` declares conditional
[`exports`](https://nodejs.org/api/packages.html#conditional-exports) for
its `"."` entry:

```json
{
  "exports": {
    ".": {
      "node": { "types": "./dist/node.d.ts", "import": "./dist/node.js" },
      "browser": { "types": "./dist/web.d.ts", "import": "./dist/web.js" },
      "default": { "types": "./dist/web.d.ts", "import": "./dist/web.js" }
    }
  }
}
```

- Node's own module resolver (and any bundler/test runner that sets the
  `"node"` condition — e.g. Vitest's default `environment: "node"`,
  webpack's `target: "node"`) picks `dist/node.js`, which re-exports
  `@cratestack/cbor-node`.
- A browser-targeting bundler (Vite's client build, webpack's default `web`
  target, Next.js client bundles) sets the `"browser"` condition and picks
  `dist/web.js`, which re-exports `@cratestack/cbor-web`.
- `"default"` is the fallback for resolvers that set neither condition — it
  points at the WASM build (`dist/web.js`), the more broadly portable of
  the two: it only needs `fetch`/`URL`/`WebAssembly`, all present in every
  modern JS runtime (Node, browsers, edge/worker runtimes), whereas the
  Node build depends on a native `.node` binary that plainly cannot load
  outside Node. The same reasoning is why the package-level `"main"`/`"types"`
  fields (read by tooling that ignores `exports` entirely) also point at
  the WASM build.

## Escape hatch: explicit subpaths

Conditional `exports` resolution isn't perfectly consistent across every
bundler/SSR setup — some SSR frameworks resolve the `"node"` condition even
for a module that will also ship to the client (their SSR build sets
`"node"` because the code *runs* under Node during the server render, even
though the same module graph gets bundled for the browser too). When
automatic resolution doesn't do what you need, import the platform build
directly:

```ts
// Force the native Node build, regardless of which condition the
// resolver would otherwise pick:
import { createCborCodec } from "@cratestack/cbor/node";

// Force the WASM build — e.g. from an SSR entry point whose bundler
// resolves "node" but whose output actually needs to run in the browser:
import { createCborCodec } from "@cratestack/cbor/web";
```

Both subpaths export the exact same `createCborCodec()` shape as the root
entry point — they're what the root entry point's `"node"`/`"browser"`
conditions point at internally, just addressable directly.

## Sync vs. async: one uniform API

`@cratestack/cbor-node`'s own export (`cborCodec`) is a plain synchronous
object — native N-API modules load synchronously via Node's
`require`/ESM interop, so there's no async step to wait on.
`@cratestack/cbor-web`'s own export is necessarily async
(`createCborCodec(): Promise<CratestackRpcCodec>`) — one-time WASM
instantiation has no synchronous equivalent in a browser.

This package normalizes **both** platforms to the same async-factory
shape: `createCborCodec()` everywhere, always returning a `Promise`. On
Node that Promise resolves immediately (the underlying codec is already
loaded and synchronous by the time the module's `import` completes) — it's
strictly unnecessary async ceremony for that platform alone, but it buys a
single, genuinely platform-agnostic call site. `await createCborCodec()`
behaves identically no matter which condition resolved; nothing in
consuming code needs to branch on environment. Once resolved, every
`encode`/`decode` call is synchronous on both platforms — the async cost,
real or nominal, is paid exactly once.

The alternative — exposing the platform difference directly (a sync export
on Node, an async factory in the browser) and documenting it — was
considered and rejected: it would force every consumer of this umbrella
package to either branch on environment or always `await` a value that's
sometimes a real async boundary and sometimes not, which defeats the
purpose of an umbrella package that's supposed to hide exactly that kind
of platform difference.

## Error handling

Malformed CBOR input on `decode`, or a value `encode` can't represent,
throws a catchable JS `Error` on both platforms — see
`@cratestack/cbor-node`'s and `@cratestack/cbor-web`'s own READMEs for the
platform-specific mechanism (native panic-to-exception conversion vs. a
non-poisoning WASM trap boundary).

## Null handling

Both backends translate every JSON `null` (top-level or nested) to the
real CBOR null byte (`0xf6`), matching `cratestack-codec-cbor`'s own
`Option::None` encoding — not the empty-array quirk (`0x80`) some CBOR
backends produce for a bare unit type. See either platform package's
README for detail.

## Contributing to this package (monorepo-local dev)

This package's own build (`tsc`) is pure TypeScript — it never needs a Rust
or wasm toolchain. But its `dependencies` are `@cratestack/cbor-node` and
`@cratestack/cbor-web`, which do. Within this monorepo, this package's
turbo `build` task deliberately does **not** build those two siblings
automatically (see `turbo.json`'s `@cratestack/cbor#build` override) — a
default dependency edge would force every toolchain-free environment
(including this repo's own `js` CI job) to compile native/wasm code just
to typecheck a thin re-export wrapper.

Practical effect: `pnpm turbo run build test lint --filter='./packages/cratestack-cbor'`
type-checks cleanly and passes lint anywhere, but its Node round-trip
tests (`tests/node.test.ts`) only run for real if `@cratestack/cbor-node`
is *already* built — otherwise they skip (not fail) with a console
warning. For real coverage while developing, build the sibling first:

```sh
pnpm turbo run build --filter='./packages/cratestack-cbor-node'
pnpm turbo run build test lint --filter='./packages/cratestack-cbor'
```

Or just run the full unfiltered `pnpm turbo run build test lint` at the
repo root — that builds every package as its own top-level target
regardless of this edge, so the siblings are present and the tests run
for real.

## See Also

- `@cratestack/cbor-node` — the Node/native implementation (napi-rs).
- `@cratestack/cbor-web` — the browser/WASM implementation (wasm-bindgen).
- `crates/cratestack-codec-cbor` — the underlying, unchanged Rust codec
  both implementations wrap.

## License

MIT
