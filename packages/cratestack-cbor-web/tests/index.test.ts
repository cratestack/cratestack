import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import type { CratestackRpcCodec } from "@cratestack/ts-types";
import { beforeAll, describe, expect, it } from "vitest";

// This suite loads the actual BUILT package (`dist/`, produced by `pnpm
// run build` — wasm-pack + tsc), not `src/`, to prove the shipped
// artifact itself works, not just the TypeScript source.
//
// `dist/index.js` calls the wasm-bindgen glue's default `init()`, which
// resolves the `.wasm` asset via `new URL('cratestack_cbor_wasm_bg.wasm',
// import.meta.url)` and `fetch()`s it. That's exactly right for a real
// bundler (Vite/webpack resolve and rewrite that URL to something their
// dev server/output serves over http, and browser `fetch` handles it) —
// see the vite-example integration check for that path. Plain Node's
// `fetch`, though, does not support `file://` URLs at all (throws "fetch
// failed"), and `import.meta.url` for a module loaded straight off disk
// under Node/vitest *is* a `file://` URL. So this suite installs a
// narrowly-scoped shim that only intercepts `file://` requests (reading
// the file straight off disk) and forwards everything else to the real
// `fetch` — good enough to exercise `createCborCodec()` unmodified under
// plain Node, without needing a browser/jsdom environment or a second,
// Node-target wasm-pack build just for tests.
const realFetch = globalThis.fetch;
globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
  const url = input instanceof Request ? input.url : input.toString();
  if (url.startsWith("file://")) {
    const bytes = await readFile(fileURLToPath(url));
    return new Response(bytes, {
      status: 200,
      headers: { "content-type": "application/wasm" },
    });
  }
  return realFetch(url, init);
}) as typeof fetch;

const { createCborCodec } = await import("../dist/index.js");

describe("createCborCodec (built package)", () => {
  let codec: CratestackRpcCodec;

  beforeAll(async () => {
    codec = await createCborCodec();
  });

  it("exposes application/cbor as its content type", () => {
    expect(codec.contentType).toBe("application/cbor");
  });

  it("round-trips a plain object through encode/decode synchronously", () => {
    const input = { name: "cratestack", tags: ["cool", "stack"], count: 2 };

    // No `await` on either call — the whole point of the async factory
    // is that encode/decode are synchronous once createCborCodec()
    // resolves.
    const encoded = codec.encode(input);
    expect(encoded).toBeInstanceOf(Uint8Array);
    const decoded = codec.decode(encoded as Uint8Array);

    expect(decoded).toEqual(input);
  });

  it("encodes Option::None-equivalent (null) as the single CBOR null byte 0xf6", () => {
    // The exact byte cratestack-codec-cbor's own test asserts for
    // `Option::<String>::None` — see crates/cratestack-codec-cbor/
    // src/lib.rs's `optional_none_round_trips_as_cbor_null`.
    const bytes = codec.encode(null) as Uint8Array;
    expect(Array.from(bytes)).toEqual([0xf6]);
    expect(codec.decode(bytes)).toBeNull();
  });

  it("round-trips a null field nested inside an object", () => {
    const input = { note: null, count: 1 };
    const bytes = codec.encode(input) as Uint8Array;
    const decoded = codec.decode(bytes);
    expect(decoded).toEqual(input);
  });

  it("rejects malformed CBOR with a catchable Error, not a crash", () => {
    const malformed = new Uint8Array([0xff, 0x00, 0x01]);
    expect(() => codec.decode(malformed)).toThrow();
  });

  it("keeps working after a decode error — the module isn't poisoned", () => {
    const malformed = new Uint8Array([0xff, 0x00, 0x01]);
    expect(() => codec.decode(malformed)).toThrow();

    // If a wasm trap had corrupted the module's linear memory, this call
    // (unrelated to the failing one) would also fail or crash the
    // process instead of returning normally.
    const bytes = codec.encode({ still: "alive" }) as Uint8Array;
    expect(codec.decode(bytes)).toEqual({ still: "alive" });
  });

  it("satisfies the CratestackRpcCodec shape end to end (type-level via structural assignment)", () => {
    // Compiles only if the returned object structurally matches
    // CratestackRpcCodec — a type-level check, not just runtime shape.
    const typed: CratestackRpcCodec = codec;
    expect(typed.encode).toBeTypeOf("function");
    expect(typed.decode).toBeTypeOf("function");
  });

  it("cross-language: decodes bytes produced by cratestack-codec-cbor's own Rust fixtures", () => {
    // Bytes for `vec!["cool", "stack"]`, the exact fixture
    // cratestack-codec-cbor's own `round_trips_value` test encodes —
    // captured by running that test and dumping `bytes` (minicbor-serde
    // is deterministic for this shape: a 2-element array of short
    // strings). Kept here (rather than a shared fixture file) since
    // #286 (@cratestack/cbor-node) has no branch/PR yet to share one
    // with — see issue #287's task list.
    const rustEncodedCoolStack = new Uint8Array([
      0x82, 0x64, 0x63, 0x6f, 0x6f, 0x6c, 0x65, 0x73, 0x74, 0x61, 0x63, 0x6b,
    ]);
    expect(codec.decode(rustEncodedCoolStack)).toEqual(["cool", "stack"]);

    // And the reverse: what this package encodes for the same value
    // must be byte-identical to what the Rust codec produces.
    const encoded = codec.encode(["cool", "stack"]) as Uint8Array;
    expect(Array.from(encoded)).toEqual(Array.from(rustEncodedCoolStack));
  });
});

describe("binary data (cratestack#783)", () => {
  // Mirrors `packages/cratestack-cbor-node/tests/codec.test.ts`'s suite
  // of the same name: the two builds wrap the same Rust codec through
  // different FFI layers (wasm-bindgen here, N-API there) and must agree
  // byte-for-byte on binary payloads, or a TypeScript client would speak
  // a different wire depending on where it runs.
  //
  // `48` is CBOR major type 2, length 8. Before the fix a `Uint8Array`
  // encoded as a map of index→value, which no generated Rust `Bytes`
  // field can decode.
  let codec: CratestackRpcCodec;

  const eightBytes = [1, 2, 3, 4, 5, 6, 7, 8];
  const BYTE_STRING = [0x48, 1, 2, 3, 4, 5, 6, 7, 8];

  beforeAll(async () => {
    codec = await createCborCodec();
  });

  it("encodes a Uint8Array as a CBOR byte string, not a map of indices", () => {
    const bytes = codec.encode(new Uint8Array(eightBytes)) as Uint8Array;
    expect(Array.from(bytes)).toEqual(BYTE_STRING);
  });

  it("encodes an ArrayBuffer as a CBOR byte string", () => {
    const bytes = codec.encode(new Uint8Array(eightBytes).buffer) as Uint8Array;
    expect(Array.from(bytes)).toEqual(BYTE_STRING);
  });

  it("encodes an empty Uint8Array as the zero-length byte string", () => {
    // 0x40 — not 0x80 (empty array) and not 0xa0 (empty map).
    expect(Array.from(codec.encode(new Uint8Array()) as Uint8Array)).toEqual([0x40]);
  });

  it("decodes a CBOR byte string back to a Uint8Array", () => {
    const decoded = codec.decode(new Uint8Array(BYTE_STRING));
    expect(decoded).toBeInstanceOf(Uint8Array);
    expect(Array.from(decoded as Uint8Array)).toEqual(eightBytes);
  });

  it("round-trips a Uint8Array nested inside an object", () => {
    const bytes = codec.encode({
      nonce: new Uint8Array([0xde, 0xad]),
      label: "mailbox",
    }) as Uint8Array;
    const decoded = codec.decode(bytes) as { nonce: unknown; label: string };
    expect(decoded.nonce).toBeInstanceOf(Uint8Array);
    expect(Array.from(decoded.nonce as Uint8Array)).toEqual([0xde, 0xad]);
    expect(decoded.label).toBe("mailbox");
  });

  it("leaves a plain number[] as a CBOR array, in both directions", () => {
    // The `Array.from(bytes)` workaround callers write today must keep
    // behaving exactly as before — an untyped value carries no schema,
    // so nothing here may guess that an integer array "meant" bytes.
    const bytes = codec.encode(eightBytes) as Uint8Array;
    expect(Array.from(bytes)).toEqual([0x88, ...eightBytes]);
    expect(codec.decode(bytes)).toEqual(eightBytes);
  });
});
