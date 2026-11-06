// Compact wire keys through the real N-API addon. The artifact is the one
// `crates/cratestack-cbor-napi/src/compact_keys/tests.rs` pins: one op,
// `shop.put`, whose request type is `{amount: 0, id: 1}` and whose response
// is opaque.
import { describe, expect, it } from "vitest";
import { createCompactKeys } from "../src/index.js";

const ARTIFACT =
  "8301818466616d6f756e74006269640081846873686f702e707574582007070707070707070707070707070707070707070707070707070707070707070100";

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

// {"id":"a","amount":5} and its compact form {1:"a",0:5}.
const NAMED = hexToBytes("a2626964616166616d6f756e7405");
const COMPACT = hexToBytes("a20161610005");

describe("createCompactKeys (native addon)", () => {
  it("rewrites CBOR keys to integers and back", async () => {
    const keys = await createCompactKeys(hexToBytes(ARTIFACT));
    expect(Array.from(keys.toCompact(NAMED, "shop.put", "request"))).toEqual(Array.from(COMPACT));
    expect(Array.from(keys.fromCompact(COMPACT, "shop.put", "request"))).toEqual(Array.from(NAMED));
  });

  it("rewrites JSON keys to decimal strings and back", async () => {
    const keys = await createCompactKeys(hexToBytes(ARTIFACT));
    const named = '{"id":"a","amount":5}';
    const compact = '{"1":"a","0":5}';
    expect(keys.toCompactJson(named, "shop.put", "request")).toBe(compact);
    expect(keys.fromCompactJson(compact, "shop.put", "request")).toBe(named);
  });

  it("passes an opaque direction through untouched", async () => {
    const keys = await createCompactKeys(hexToBytes(ARTIFACT));
    expect(Array.from(keys.toCompact(NAMED, "shop.put", "response"))).toEqual(Array.from(NAMED));
  });

  it("names the Cratestack-Contract selector of the op, or undefined", async () => {
    const keys = await createCompactKeys(hexToBytes(ARTIFACT));
    expect(keys.selector("shop.put")).toBe("BwcHBwcHBwc");
    expect(keys.selector("shop.missing")).toBeUndefined();
  });

  it("throws a catchable Error on bad input", async () => {
    const keys = await createCompactKeys(hexToBytes(ARTIFACT));
    expect(() => keys.toCompact(NAMED, "shop.missing", "request")).toThrow(/no compact key table/);
    expect(() => keys.toCompact(NAMED, "shop.put", "sideways" as never)).toThrow(/direction/);
    expect(() => keys.toCompact(new Uint8Array([0x1b]), "shop.put", "request")).toThrow();
    expect(() => keys.fromCompactJson("{", "shop.put", "request")).toThrow();
    await expect(createCompactKeys(new Uint8Array([1, 2, 3]))).rejects.toThrow();
  });
});
