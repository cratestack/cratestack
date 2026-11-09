// COSE through the real N-API addon (issue #1010): the shared known-answer
// vectors of `crates/cratestack-cose/tests/vectors`, the same bytes the Rust
// native tests, the wasm tests and `@cratestack/cbor-web/cose` produce.
import { type CoseVectors, defineCoseConformance } from "@cratestack/ts-types/cose-conformance";
import { describe, expect, it } from "vitest";
import keys from "../../../crates/cratestack-cose/tests/vectors/keys.json" with { type: "json" };
import unary from "../../../crates/cratestack-cose/tests/vectors/unary.json" with { type: "json" };
import * as cose from "../src/cose.js";

const vectors = { keys, unary } as unknown as CoseVectors;

defineCoseConformance(cose, vectors, { describe, it, expect });

describe("@cratestack/cbor-node/cose", () => {
  it("throws CoseFailure for what napi itself refuses (a binding of the wrong shape)", async () => {
    const client = await cose.createCoseClient({
      signer: { type: "ed25519-seed", seed: new Uint8Array(32) },
      serverKeys: [],
      audience: "a",
    });
    await expect(
      client.sealRequest(new Uint8Array(), { method: 1 } as unknown as never),
    ).rejects.toBeInstanceOf(cose.CoseFailure);
  });

  it("an external signer is asked for the to-be-signed bytes exactly once per seal", async () => {
    const seen: Uint8Array[] = [];
    const key = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, [
      "sign",
      "verify",
    ]);
    const publicKey = new Uint8Array(await crypto.subtle.exportKey("raw", key.publicKey));
    const client = await cose.createCoseClient({
      signer: {
        type: "external",
        alg: "esp256",
        publicKey,
        sign: async (tbs) => {
          seen.push(tbs);
          return new Uint8Array(
            await crypto.subtle.sign(
              { name: "ECDSA", hash: "SHA-256" },
              key.privateKey,
              tbs as BufferSource,
            ),
          );
        },
      },
      serverKeys: [{ alg: "esp256", bytes: publicKey }],
      audience: "payments",
    });
    const binding = {
      method: "POST",
      route: "model.Payment.create",
      pathParams: [],
      contractSha: new Uint8Array(32).fill(7),
    };
    const [a, b] = await Promise.all([
      client.sealRequest(new Uint8Array([0xa0]), binding),
      client.sealRequest(new Uint8Array([0xa0]), binding),
    ]);
    expect(seen).toHaveLength(2);
    expect(a).not.toEqual(b);
    // The to-be-signed bytes are the COSE Sig_structure: ["Signature1", ...].
    expect(Array.from(seen[0]?.slice(0, 12) ?? [])).toEqual([
      0x84, 0x6a, 0x53, 0x69, 0x67, 0x6e, 0x61, 0x74, 0x75, 0x72, 0x65, 0x31,
    ]);
  });
});
