import { describe, expect, it } from "vitest";
import { CborSeqBoundaryScanner, MalformedCborSeqError } from "../src/cbor-seq.js";

describe("streaming CBOR validation", () => {
  it("waits for complete text and simple values, then validates", () => {
    const text = new CborSeqBoundaryScanner();
    expect(text.feedChunk(Uint8Array.from([0x62, 0xc2]))).toEqual([]);
    expect(text.feedChunk(Uint8Array.from([0xa2]))).toEqual([Uint8Array.from([0x62, 0xc2, 0xa2])]);
    const invalid = new CborSeqBoundaryScanner();
    expect(invalid.feedChunk(Uint8Array.from([0x62, 0xc0]))).toEqual([]);
    expect(() => invalid.feedChunk(Uint8Array.from([0x80]))).toThrow(MalformedCborSeqError);
    const simple = new CborSeqBoundaryScanner();
    expect(simple.feedChunk(Uint8Array.from([0xf8]))).toEqual([]);
    expect(simple.feedChunk(Uint8Array.from([0x20]))).toEqual([Uint8Array.from([0xf8, 0x20])]);
    const invalidSimple = new CborSeqBoundaryScanner();
    expect(invalidSimple.feedChunk(Uint8Array.from([0xf8]))).toEqual([]);
    expect(() => invalidSimple.feedChunk(Uint8Array.from([0x00]))).toThrow(MalformedCborSeqError);
  });
});
