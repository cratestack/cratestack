import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { type CoseVectors, defineCoseConformance } from "@cratestack/ts-types/cose-conformance";
import { describe, expect, it } from "vitest";

// Loads the BUILT package (`dist/`), like index.test.ts, with the same
// `file://` fetch shim for the `.wasm` asset (see there for why).
const realFetch = globalThis.fetch;
globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
  const url = input instanceof Request ? input.url : input.toString();
  if (url.startsWith("file://")) {
    return new Response(await readFile(fileURLToPath(url)), {
      status: 200,
      headers: { "content-type": "application/wasm" },
    });
  }
  return realFetch(url, init);
}) as typeof fetch;

const cose = await import("../dist/cose.js");

const vectorFile = async (name: string): Promise<unknown> =>
  JSON.parse(
    await readFile(
      new URL(`../../../crates/cratestack-cose/tests/vectors/${name}.json`, import.meta.url),
      "utf8",
    ),
  );
const vectors = {
  keys: await vectorFile("keys"),
  unary: await vectorFile("unary"),
} as CoseVectors;

// The shared known-answer vectors, through the WASM build of cratestack-cose.
defineCoseConformance(cose, vectors, { describe, it, expect });

describe("@cratestack/cbor-web/cose", () => {
  it("derives the same kid from the same key in every client", async () => {
    const a = await cose.createCoseClient({
      signer: { type: "ed25519-seed", seed: new Uint8Array(32) },
      serverKeys: [],
      audience: "a",
    });
    const b = await cose.createCoseClient({
      signer: { type: "ed25519-seed", seed: new Uint8Array(32) },
      serverKeys: [],
      audience: "b",
    });
    expect(a.kid).toEqual(b.kid);
  });
});
