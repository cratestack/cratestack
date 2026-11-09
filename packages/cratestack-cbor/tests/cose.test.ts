// `@cratestack/cbor/cose` resolves, under Node, to the native COSE binding
// (issue #1010) and never touches the browser's WASM. Imported through the
// package's own name so a mistyped `exports` entry fails here; skipped (with a
// warning, like node.test.ts) when @cratestack/cbor-node is not built.
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

const cborNodePkgDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../node_modules/@cratestack/cbor-node",
);
const built =
  existsSync(path.join(cborNodePkgDir, "dist/cose.js")) &&
  existsSync(path.join(cborNodePkgDir, "native.mjs"));
if (!built) {
  console.warn("@cratestack/cbor-node isn't built — skipping @cratestack/cbor/cose's Node suite.");
}

describe.skipIf(!built)("@cratestack/cbor/cose (Node)", () => {
  it("seals with the native binding and never fetches a wasm module", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const { createCoseClient, CoseFailure } = await import("@cratestack/cbor/cose");
    const client = await createCoseClient({
      signer: { type: "hmac", alg: "hmac256-64", secret: new Uint8Array(32).fill(1) },
      serverKeys: [],
      audience: "payments",
    });
    expect(client.mode).toBe("mac0");
    const sealed = await client.sealRequest(new Uint8Array([0xa0]), {
      method: "POST",
      route: "model.Payment.create",
      pathParams: [],
      contractSha: new Uint8Array(32).fill(7),
    });
    // COSE_Mac0 is CBOR tag 17.
    expect(sealed[0]).toBe(0xd1);
    expect(fetchSpy).not.toHaveBeenCalled();
    await expect(
      client.sealRequest(new Uint8Array(), { method: "POST" } as never),
    ).rejects.toBeInstanceOf(CoseFailure);
    fetchSpy.mockRestore();
  });
});
