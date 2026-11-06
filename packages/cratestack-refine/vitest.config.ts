import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    // The generated RPC runtime imports `@cratestack/cbor` statically, and
    // on Node that resolves to an napi addon this package's CI job never
    // builds. The tests speak JSON (`codec: jsonRpcCodec`), so a stub stands
    // in; see `tests/support/cbor-stub.ts`.
    alias: {
      "@cratestack/cbor": fileURLToPath(new URL("./tests/support/cbor-stub.ts", import.meta.url)),
    },
  },
  test: {
    // See tests/support/assert-fixture-present.ts's own doc comment:
    // fails the whole run loudly (not a silent skip) when the real
    // generated TypeScript client fixture this package's tests drive
    // hasn't been produced yet (`just refine-fixture`).
    globalSetup: ["./tests/support/assert-fixture-present.ts"],
  },
});
