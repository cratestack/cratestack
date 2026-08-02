// Type-level proof that `cborCodec` structurally satisfies
// `CratestackRpcCodec`, per issue #286's acceptance criterion: "verified
// by a TypeScript type-level test/assertion, not just runtime behavior."
//
// The assignment below is checked at compile time by
// `tsc -p tsconfig.test.json --noEmit`, which `pnpm test` runs before
// `vitest run` (see package.json's "test" script) — so a shape mismatch
// fails the test command, not just an editor's red squiggle.
import type { CratestackRpcCodec } from "@cratestack/ts-types";
import { describe, expect, it } from "vitest";
import { cborCodec } from "../src/index.js";

const _typeContract: CratestackRpcCodec = cborCodec;

describe("CratestackRpcCodec structural contract", () => {
  it("cborCodec is assignable to CratestackRpcCodec (see the compile-time assertion above)", () => {
    const codec: CratestackRpcCodec = cborCodec;
    expect(codec.contentType).toBe("application/cbor");
    expect(typeof codec.encode).toBe("function");
    expect(typeof codec.decode).toBe("function");
  });
});
