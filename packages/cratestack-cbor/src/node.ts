// Node entry point (resolved by the `"node"` condition of this package's
// root `exports["."]`, and directly importable as the `@cratestack/cbor/node`
// escape hatch — see package.json and README.md).
//
// @cratestack/cbor-node's own export (`cborCodec`, see its src/index.ts) is
// a plain synchronous object: native N-API modules load synchronously via
// Node's `require`/ESM interop, so there's no async initialization step to
// wait on. @cratestack/cbor-web's export, by contrast, is necessarily async
// (`createCborCodec()`) — one-time WASM instantiation has no synchronous
// equivalent in a browser.
//
// This package normalizes both platforms to the SAME public shape: an async
// `createCborCodec()` factory everywhere. On this (Node) side that means
// wrapping an already-available synchronous value in a resolved Promise —
// strictly unnecessary work for this platform alone, but it buys a single,
// genuinely platform-agnostic call site (`await createCborCodec()` behaves
// identically regardless of which condition resolved). The alternative —
// exposing the sync/async difference directly and documenting it — would
// force every consumer of this umbrella package to branch on environment
// (or always `await` a value that's sometimes already resolved and
// sometimes a real async boundary), which defeats the point of an umbrella
// package in the first place. See README.md's "Sync vs. async" section and
// issue #288 for the full reasoning.
import { cborCodec } from "@cratestack/cbor-node";
import type { CratestackRpcCodec } from "@cratestack/ts-types";

/**
 * Resolves immediately (no real async work — `@cratestack/cbor-node`'s
 * underlying native codec is already loaded and synchronous by the time
 * this module's `import` completes) to a {@link CratestackRpcCodec} backed
 * by the native N-API CBOR codec. Async purely for call-site parity with
 * the browser build's `createCborCodec()`, not because Node needs it.
 */
export async function createCborCodec(): Promise<CratestackRpcCodec> {
  return cborCodec;
}
