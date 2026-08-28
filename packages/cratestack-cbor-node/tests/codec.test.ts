// Verifies the native N-API addon (crates/cratestack-cbor-napi) actually
// loads inside Node and behaves correctly through the real FFI boundary —
// this suite runs against the compiled `.node` binary built by
// `pnpm run build:napi`, not a mock. Run `pnpm run build:napi` (or
// `build`) before `vitest run` if `native.mjs`/`*.node` aren't present
// yet.
import { describe, expect, it } from "vitest";
import { cborCodec } from "../src/index.js";

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

describe("cborCodec contract", () => {
  it("declares application/cbor as its content type", () => {
    expect(cborCodec.contentType).toBe("application/cbor");
  });

  it("round-trips a JSON-shaped value through encode/decode", () => {
    const value = { cratestack: ["cool", "stack"], n: 42, ok: true };
    const bytes = cborCodec.encode(value) as Uint8Array;
    expect(cborCodec.decode(new Uint8Array(bytes))).toEqual(value);
  });

  it("throws a catchable JS error on malformed CBOR input instead of crashing", () => {
    // 0x1b announces an 8-byte unsigned integer but supplies none —
    // truncated input CborCodec/minicbor rejects with an error. Proves
    // acceptance criterion: "malformed CBOR bytes on decode produce a
    // catchable JS error, not a native crash/panic".
    const malformed = new Uint8Array([0x1b]);
    expect(() => cborCodec.decode(malformed)).toThrow();
  });
});

describe("null handling (the documented minicbor-serde quirk)", () => {
  it("encodes top-level null as the real CBOR null byte (0xf6), not the empty-array quirk (0x80)", () => {
    // The single most important test per issue #286: cratestack-codec-cbor's
    // own Rust test suite (crates/cratestack-codec-cbor/src/lib.rs,
    // `optional_none_round_trips_as_cbor_null`) asserts
    // `codec.encode(&Option::<String>::None) == [0xf6]`. This asserts the
    // exact same wire byte through the compiled Node addon instead.
    const bytes = cborCodec.encode(null) as Uint8Array;
    expect(Array.from(bytes)).toEqual([0xf6]);
    expect(cborCodec.decode(bytes)).toBeNull();
  });

  it("preserves null nested inside objects and arrays through a round trip", () => {
    const value = { a: null, b: [1, null, "x"] };
    const bytes = cborCodec.encode(value) as Uint8Array;
    expect(cborCodec.decode(bytes)).toEqual(value);
  });
});

describe("cross-language fixtures (byte-identical to the Rust CborCodec)", () => {
  // These exact hex strings are independently asserted by
  // crates/cratestack-cbor-napi's own
  // `fixture_bytes_shared_with_the_js_cross_language_test_stay_correct`
  // Rust test, computed directly from cratestack-codec-cbor's CborCodec
  // (the same codec this package wraps, unmodified). Two independent
  // assertions of the same wire bytes from both ends of the FFI boundary
  // is what proves "byte-identical to the Rust CborCodec on the same
  // input" — not just that this package round-trips against itself.
  it("encode(['cool', 'stack']) matches the Rust-derived fixture bytes", () => {
    const bytes = cborCodec.encode(["cool", "stack"]) as Uint8Array;
    expect(bytesToHex(bytes)).toBe("8264636f6f6c65737461636b");
  });

  it("decodes the Rust-derived fixture bytes back to the original object", () => {
    const bytes = hexToBytes("a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5");
    expect(cborCodec.decode(bytes)).toEqual({
      cratestack: ["cool", "stack"],
      n: 42,
      ok: true,
    });
  });

  it("decodes the Rust-derived nested-null fixture bytes correctly", () => {
    const bytes = hexToBytes("a26161f661628301f66178");
    expect(cborCodec.decode(bytes)).toEqual({ a: null, b: [1, null, "x"] });
  });
});

describe("binary data (cratestack#783)", () => {
  // `@cratestack/cbor` used to serialise a `Uint8Array` as a CBOR *map*
  // of index→value — `a8 6130 01 6131 02 …` — which no generated Rust
  // `Bytes` field (a `Vec<u8>`) can decode, so a request carrying one
  // failed at the codec with `400 invalid_argument` and never reached the
  // handler. Callers had to hand-write `Array.from(bytes)` at every call
  // site, at roughly twice the wire cost of the byte string they wanted.
  //
  // `48` below is CBOR major type 2, length 8 — the shape `serde_cbor`,
  // `ciborium` and `minicbor` all emit for binary data.
  const eightBytes = [1, 2, 3, 4, 5, 6, 7, 8];
  const BYTE_STRING = "480102030405060708";

  it("encodes a Uint8Array as a CBOR byte string, not a map of indices", () => {
    const bytes = cborCodec.encode({ p: new Uint8Array(eightBytes) }) as Uint8Array;
    expect(bytesToHex(bytes)).toBe(`a16170${BYTE_STRING}`);
  });

  it("encodes a Node Buffer as a CBOR byte string too", () => {
    // `Buffer` is a `Uint8Array` subclass and Node-API reports it with
    // the same `uint8` typed-array element type, so it takes the same
    // path — worth pinning, since it is what `fs`/`crypto` hand callers.
    // Reached through `globalThis` because this package deliberately has
    // no `@types/node` dependency (it targets browsers too).
    const { Buffer } = globalThis as unknown as {
      Buffer: { from(data: number[]): Uint8Array };
    };
    const bytes = cborCodec.encode(Buffer.from(eightBytes)) as Uint8Array;
    expect(bytesToHex(bytes)).toBe(BYTE_STRING);
  });

  it("encodes an ArrayBuffer as a CBOR byte string", () => {
    const bytes = cborCodec.encode(new Uint8Array(eightBytes).buffer) as Uint8Array;
    expect(bytesToHex(bytes)).toBe(BYTE_STRING);
  });

  it("honours a subarray's own window rather than the whole backing buffer", () => {
    // `43 030405` — three bytes, starting at the view's byte offset. A
    // naive read of the backing `ArrayBuffer` would have produced all
    // eight.
    const bytes = cborCodec.encode(new Uint8Array(eightBytes).subarray(2, 5)) as Uint8Array;
    expect(bytesToHex(bytes)).toBe("43030405");
  });

  it("encodes an empty Uint8Array as the zero-length byte string", () => {
    // 0x40 — not 0x80 (empty array) and not 0xa0 (empty map).
    expect(bytesToHex(cborCodec.encode(new Uint8Array()) as Uint8Array)).toBe("40");
  });

  it("decodes a CBOR byte string back to a Uint8Array", () => {
    // The symmetric half the issue also asks for: a server sending a
    // `Bytes` field as major type 2 must be readable here.
    const decoded = cborCodec.decode(hexToBytes(BYTE_STRING));
    expect(decoded).toBeInstanceOf(Uint8Array);
    expect(Array.from(decoded as Uint8Array)).toEqual(eightBytes);
  });

  it("round-trips a Uint8Array nested inside an object", () => {
    const bytes = cborCodec.encode({
      nonce: new Uint8Array([0xde, 0xad]),
      label: "mailbox",
    }) as Uint8Array;
    const decoded = cborCodec.decode(bytes) as { nonce: unknown; label: string };
    expect(decoded.nonce).toBeInstanceOf(Uint8Array);
    expect(Array.from(decoded.nonce as Uint8Array)).toEqual([0xde, 0xad]);
    expect(decoded.label).toBe("mailbox");
  });

  it("leaves a plain number[] as a CBOR array, in both directions", () => {
    // The `Array.from(bytes)` workaround callers write today has to keep
    // behaving exactly as before: an untyped value carries no schema, so
    // nothing here may guess that an integer array "meant" bytes. The
    // leniency that lets a server accept both shapes lives on the Rust
    // side, where the schema says the field is `Bytes`.
    const bytes = cborCodec.encode({ p: eightBytes }) as Uint8Array;
    expect(bytesToHex(bytes)).toBe("a16170880102030405060708");
    expect(cborCodec.decode(bytes)).toEqual({ p: eightBytes });
  });

  it("leaves other typed arrays on their previous path", () => {
    // Only `Uint8Array` and `ArrayBuffer` map to bytes — exactly the set
    // `serde-wasm-bindgen` recognises on the `@cratestack/cbor-web` side,
    // matched here so a client encodes the same payload in either
    // runtime. An `Int32Array` must not be reinterpreted as its
    // little-endian bytes, and a `Uint8ClampedArray` is deliberately left
    // out despite its byte-sized elements, because the web build cannot
    // recognise one (it is not a `Uint8Array` subclass).
    //
    // What the two builds do with an *unsupported* shape still differs,
    // and did before this change: node degrades it to an object of
    // indices (asserted below), the web build rejects it outright.
    // Neither is a shape to rely on — convert to a `Uint8Array` first.
    for (const input of [new Int32Array([1, 2]), new Uint8ClampedArray([1, 2])]) {
      const decoded = cborCodec.decode(cborCodec.encode(input) as Uint8Array);
      expect(decoded).not.toBeInstanceOf(Uint8Array);
      expect(decoded).toEqual({ "0": 1, "1": 2 });
    }
  });
});
