// Real vitest proof that a schema `Bytes` field is a `Uint8Array` on both
// sides of a generated client and still reaches the wire as the integer
// array the server actually accepts — run against the generated
// `bytes_round_trip.cstack` package (`crates/cratestack-client-typescript/
// tests/fixtures/bytes_round_trip.cstack`), not asserted as
// generated-text-contains-X in Rust. Copied alongside a generated package
// by `tests/bytes_round_trip.rs`, mirroring `decimal_round_trip.rs`'s
// "generate a real package, `npm install`, run real vitest" pattern.
//
// The two halves this covers are the two that can silently corrupt data
// rather than fail loudly:
//
//   * decode — an integer array becomes a `Uint8Array` only where the
//     *schema* says the field is `Bytes`. An `Int[]` field is the same
//     `number[]` on the wire and must come back untouched.
//   * encode — `JSON.stringify` turns a `Uint8Array` into an index-keyed
//     object (`{"0":1}`) that no server-side `Vec<u8>` can decode, so the
//     JSON paths have to normalise first. Node's `Buffer` is the sharp
//     edge: it is a `Uint8Array` subclass (assignable to a generated
//     `Bytes` field) whose own `toJSON` would otherwise win.
import { describe, expect, it } from "vitest";
import { encodeBinaryAsJson, reviveWireFields, reviveWireScalar } from "./src/models.js";

describe("decode: the wire integer array becomes a Uint8Array", () => {
  it("revives a required Bytes field", () => {
    const revived = reviveWireFields(
      { id: "s_1", label: "one", payload: [1, 2, 3, 4] },
      "Sample",
    ) as { label: string; payload: unknown };

    expect(revived.payload).toBeInstanceOf(Uint8Array);
    expect(Array.from(revived.payload as Uint8Array)).toEqual([1, 2, 3, 4]);
    expect(revived.label).toBe("one");
  });

  it("leaves a null nullable Bytes field null rather than making it empty bytes", () => {
    // `null` and a zero-length `Uint8Array` are different values, and the
    // generated field type (`Uint8Array | null`) keeps them apart.
    const revived = reviveWireFields({ signature: null }, "Sample") as { signature: unknown };
    expect(revived.signature).toBeNull();
  });

  it("revives a Bytes[] field element-wise, not as one flat byte array", () => {
    const revived = reviveWireFields(
      { chunks: [[1, 2], [3]] },
      "Sample",
    ) as { chunks: unknown };

    const chunks = revived.chunks as unknown[];
    expect(chunks).toHaveLength(2);
    expect(chunks[0]).toBeInstanceOf(Uint8Array);
    expect(Array.from(chunks[0] as Uint8Array)).toEqual([1, 2]);
    expect(Array.from(chunks[1] as Uint8Array)).toEqual([3]);
  });

  it("does NOT revive an Int[] field that looks identical on the wire", () => {
    // The collision the schema-driven registry exists to prevent:
    // `readings: Int[]` and `payload: Bytes` are both `number[]` decoded.
    // Only the schema can tell them apart.
    const revived = reviveWireFields(
      { payload: [1, 2], readings: [1, 2] },
      "Sample",
    ) as { payload: unknown; readings: unknown };

    expect(revived.payload).toBeInstanceOf(Uint8Array);
    expect(revived.readings).not.toBeInstanceOf(Uint8Array);
    expect(revived.readings).toEqual([1, 2]);
  });

  it("distinguishes an empty Bytes from an empty Bytes[] using the schema, not the value", () => {
    // Both are `[]` on the wire — structurally identical, so a runtime
    // guess would have to be wrong for one of them. This is the case the
    // arity split in the shape registry exists for.
    const revived = reviveWireFields({ payload: [], chunks: [] }, "Sample") as {
      payload: unknown;
      chunks: unknown;
    };

    expect(revived.payload).toBeInstanceOf(Uint8Array);
    expect((revived.payload as Uint8Array).length).toBe(0);
    expect(Array.isArray(revived.chunks)).toBe(true);
    expect(revived.chunks).toEqual([]);
  });

  it("revives Bytes nested in a type block, via that type's own shape", () => {
    const revived = reviveWireFields({ nonce: [9, 9], note: "hi" }, "Envelope") as {
      nonce: unknown;
      note: string;
    };
    expect(revived.nonce).toBeInstanceOf(Uint8Array);
    expect(revived.note).toBe("hi");
  });

  it("revives every item of an array response (the list() shape)", () => {
    const revived = reviveWireFields(
      [{ payload: [1] }, { payload: [2] }],
      "Sample",
    ) as Array<{ payload: Uint8Array }>;

    expect(revived[0].payload).toBeInstanceOf(Uint8Array);
    expect(revived[1].payload).toBeInstanceOf(Uint8Array);
  });
});

describe("decode: bare scalar returns (reviveWireScalar)", () => {
  it("revives a bare Bytes return", () => {
    const revived = reviveWireScalar([1, 2, 3], "bytes");
    expect(revived).toBeInstanceOf(Uint8Array);
    expect(Array.from(revived as Uint8Array)).toEqual([1, 2, 3]);
  });

  it("revives a bare Bytes[] return element-wise", () => {
    const revived = reviveWireScalar([[1], [2, 3]], "bytesList") as unknown[];
    expect(revived[0]).toBeInstanceOf(Uint8Array);
    expect(Array.from(revived[1] as Uint8Array)).toEqual([2, 3]);
  });

  it("keeps the empty case distinct between the two kinds", () => {
    expect(reviveWireScalar([], "bytes")).toBeInstanceOf(Uint8Array);
    expect(reviveWireScalar([], "bytesList")).toEqual([]);
  });

  it("is a no-op for an unrecognised kind rather than throwing", () => {
    const value = { untouched: true };
    expect(reviveWireScalar(value, "somethingElse")).toBe(value);
  });
});

describe("encode: a Uint8Array survives the JSON transports", () => {
  it("becomes an integer array, not JSON.stringify's index-keyed object", () => {
    // Without this normalisation the body is `{"payload":{"0":1,"1":2}}`,
    // which the server rejects — the same defect cratestack#783 fixed on
    // the CBOR side, in a different disguise.
    const body = JSON.stringify(encodeBinaryAsJson({ payload: new Uint8Array([1, 2]) }));
    expect(body).toBe('{"payload":[1,2]}');
  });

  it("normalises a Node Buffer identically, despite its own toJSON", () => {
    // `Buffer.prototype.toJSON` returns `{type:"Buffer",data:[...]}` and
    // `JSON.stringify` applies it *before* any replacer, which is why the
    // conversion is a pre-walk rather than a stringify replacer.
    const body = JSON.stringify(encodeBinaryAsJson({ payload: Buffer.from([1, 2]) }));
    expect(body).toBe('{"payload":[1,2]}');
    expect(body).not.toContain("Buffer");
  });

  it("reaches binary nested in objects and arrays", () => {
    const body = JSON.stringify(
      encodeBinaryAsJson({ extra: { nonce: new Uint8Array([7]) }, chunks: [new Uint8Array([8])] }),
    );
    expect(body).toBe('{"extra":{"nonce":[7]},"chunks":[[8]]}');
  });

  it("leaves every non-binary value untouched", () => {
    const value = { n: 1, s: "a", b: true, nul: null, arr: [1, 2], nested: { k: "v" } };
    expect(JSON.stringify(encodeBinaryAsJson(value))).toBe(JSON.stringify(value));
  });

  it("round-trips: encode for the wire, then decode back to the same bytes", () => {
    const original = new Uint8Array([1, 2, 3, 4]);
    const onTheWire = JSON.parse(JSON.stringify(encodeBinaryAsJson({ payload: original })));
    const decoded = reviveWireFields(onTheWire, "Sample") as { payload: Uint8Array };

    expect(decoded.payload).toBeInstanceOf(Uint8Array);
    expect(Array.from(decoded.payload)).toEqual(Array.from(original));
  });
});
