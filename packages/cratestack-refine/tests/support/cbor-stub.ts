/** Test-only stand-in for `@cratestack/cbor`, mapped in by
 *  `vitest.config.ts` (`resolve.alias`) and `tsconfig.typecheck.json`
 *  (`paths`).
 *
 *  The generated RPC runtime always imports `createCborCodec` from
 *  `@cratestack/cbor` at the top level; the JSON fallback mode it used to
 *  have is gone. On Node that import resolves to `@cratestack/cbor-node`'s
 *  napi addon, which the `js (@cratestack/refine)` CI job never builds, and
 *  Vite resolves a static import even when nothing calls it. These tests
 *  never need CBOR: `FakeRpcServer` speaks JSON, so `createRpcTestClient`
 *  passes `codec: jsonRpcCodec` explicitly and this factory is never
 *  reached. It throws, so a test that does reach it fails loudly instead of
 *  silently using the wrong codec. */
export function createCborCodec(): Promise<never> {
  return Promise.reject(
    new Error(
      "@cratestack/cbor is stubbed in @cratestack/refine's tests; pass `codec: jsonRpcCodec`",
    ),
  );
}
