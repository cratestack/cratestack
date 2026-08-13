// Fallback ambient type declarations for `@cratestack/cbor-node` and
// `@cratestack/cbor-web`.
//
// This package's `dependsOn` for its own `build` task (see turbo.json's
// `@cratestack/cbor#build` override) depends on @cratestack/ts-types'
// build but deliberately NOT on @cratestack/cbor-node's or
// @cratestack/cbor-web's — it does NOT force those two to be freshly
// compiled first, because their builds need Rust/wasm toolchains this
// repo's toolchain-free `js` CI job doesn't have. That means `tsc`
// compiling src/node.ts and src/web.ts sometimes runs against a
// workspace where those two siblings' own emitted `dist/*.d.ts`
// genuinely doesn't exist yet (unbuilt).
//
// TypeScript only consults an ambient `declare module` block for a given
// specifier when real file-based resolution can't find a match — when
// the siblings ARE built (local dev with the Rust/wasm toolchains, or
// this repo's dedicated `js-cbor-napi`/`js-cbor-wasm` CI jobs), their
// real, richer declarations resolve normally and these ambient
// declarations are simply unused. This file is a typecheck-time safety
// net for the toolchain-free lane, not the source of truth for either
// package's actual API — see their own `src/index.ts` for that.
declare module "@cratestack/cbor-node" {
  import type { CratestackRpcCodec } from "@cratestack/ts-types";

  export const cborCodec: CratestackRpcCodec;
}

declare module "@cratestack/cbor-web" {
  import type { CratestackRpcCodec } from "@cratestack/ts-types";

  export function createCborCodec(): Promise<CratestackRpcCodec>;
}
