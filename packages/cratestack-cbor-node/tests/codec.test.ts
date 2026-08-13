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
