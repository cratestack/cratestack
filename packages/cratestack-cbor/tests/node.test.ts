// Real-Node verification for issue #288's first acceptance criterion:
// "Given a Node project, when it imports @cratestack/cbor, then it
// resolves to the native @cratestack/cbor-node implementation and never
// touches @cratestack/cbor-web's wasm binary."
//
// Imports go through the package's own name ("@cratestack/cbor" and its
// "/node" subpath), not "../src/*.js" — self-referencing a package by its
// own name resolves through the published `exports` map (Node.js's
// self-reference resolution, since Node 12.16 / npm 7), the same
// technique @cratestack/api's own compat-reexport test uses (see
// packages/cratestack-api/tests/index.test.ts) — so this suite also
// catches a broken/mistyped `exports` entry in package.json, which a
// source-relative import would silently sidestep. This is why `test`
// depends on `build` in turbo.json: `exports` points at `./dist/*`.
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { CratestackRpcCodec } from "@cratestack/ts-types";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// This package's `cratestack-cbor#build` turbo task deliberately has no
// `^build` dependency on @cratestack/cbor-node (see turbo.json's comment
// on that override and src/ambient.d.ts) — its own Rust/napi build needs
// a toolchain this repo's toolchain-free `js` CI job doesn't set up.
// That means @cratestack/cbor-node genuinely isn't built in that job,
// and importing "@cratestack/cbor" for real there would hard-fail at
// module load (an unresolvable `import` inside the compiled
// dist/node.js, not a catchable per-call error). Rather than let that
// break the `js` job — or silently report a false green by not testing
// anything — this suite checks whether the real, built sibling is
// actually present on disk first and skips (not fails) if it isn't,
// mirroring this repo's own established convention for a prerequisite
// that's legitimately unavailable in a given CI lane (see
// CRATESTACK_TEST_DATABASE_URL's PG-integration-test skip in the
// workspace root CLAUDE.md). Real coverage still happens: locally with
// the Rust toolchain (see this package's README), and in CI's
// `js-cbor-napi` job once it's extended to also run this package's
// suite (see the PR that introduced this file for that CI change).
const cborNodePkgDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../node_modules/@cratestack/cbor-node",
);
const cborNodeDistPath = path.join(cborNodePkgDir, "dist/index.js");
// The tsc-compiled `dist/index.js` alone isn't sufficient evidence: a
// stale/partial cache restore can leave it present without the actual
// native addon (`native.mjs`, produced by a separate `napi build` step
// in this package's "build" script chain — see package.json) — hit once
// during this suite's own development. Both must exist for a real round
// trip to work.
const cborNodeIsBuilt =
  existsSync(cborNodeDistPath) && existsSync(path.join(cborNodePkgDir, "native.mjs"));

if (!cborNodeIsBuilt) {
  // A visible warning, not a silent skip — shows up in CI logs so an
  // expected toolchain-free-lane skip is easy to tell apart from a real
  // regression at a glance.
  console.warn(
    `@cratestack/cbor-node isn't built (expected at ${cborNodeDistPath}) — skipping @cratestack/cbor's Node round-trip suite. Run "pnpm turbo run build --filter='./packages/cratestack-cbor-node'" first for real coverage.`,
  );
}

// @cratestack/cbor-web's factory performs its one-time WASM init via a
// `fetch()` of the `.wasm` asset (see packages/cratestack-cbor-web/
// src/index.ts's `ensureInitialized`). If this Node-side import ever
// resolved to the browser/wasm build instead of the native one — a
// misconfigured `exports` condition order, for example — that fetch call
// would fire. Spying on `globalThis.fetch` and asserting it's never
// invoked is a real, mechanistic proof of "never touches
// @cratestack/cbor-web's wasm binary" here, not just an assertion that
// the returned values happen to look right.
let fetchSpy: ReturnType<typeof vi.fn>;

beforeEach(() => {
  fetchSpy = vi.fn(() => {
    throw new Error("fetch should not be called by the Node build of @cratestack/cbor");
  });
  vi.stubGlobal("fetch", fetchSpy);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

describe.skipIf(!cborNodeIsBuilt)(
  "@cratestack/cbor in a plain Node process (root entry point)",
  () => {
    it("resolves the `node` exports condition to the native codec, without ever calling fetch", async () => {
      const { createCborCodec } = await import("@cratestack/cbor");
      const codec: CratestackRpcCodec = await createCborCodec();

      expect(codec.contentType).toBe("application/cbor");
      expect(fetchSpy).not.toHaveBeenCalled();
    });

    it("round-trips a JSON-shaped value through encode/decode", async () => {
      const { createCborCodec } = await import("@cratestack/cbor");
      const codec = await createCborCodec();

      const value = { cratestack: ["cool", "stack"], n: 42, ok: true };
      const bytes = codec.encode(value) as Uint8Array;
      expect(codec.decode(new Uint8Array(bytes))).toEqual(value);
      expect(fetchSpy).not.toHaveBeenCalled();
    });

    it("encodes top-level null as the real CBOR null byte (0xf6), matching the Rust CborCodec", async () => {
      const { createCborCodec } = await import("@cratestack/cbor");
      const codec = await createCborCodec();

      const bytes = codec.encode(null) as Uint8Array;
      expect(Array.from(bytes)).toEqual([0xf6]);
      expect(codec.decode(bytes)).toBeNull();
    });

    it("cross-language: encode(['cool', 'stack']) matches the Rust-derived fixture bytes", async () => {
      // Same exact fixture @cratestack/cbor-node's own test suite asserts
      // (packages/cratestack-cbor-node/tests/codec.test.ts) — proves this
      // package's Node path is byte-identical to that package, not a
      // reimplementation.
      const { createCborCodec } = await import("@cratestack/cbor");
      const codec = await createCborCodec();

      const bytes = codec.encode(["cool", "stack"]) as Uint8Array;
      expect(bytesToHex(bytes)).toBe("8264636f6f6c65737461636b");
    });

    it("throws a catchable JS error on malformed CBOR input", async () => {
      const { createCborCodec } = await import("@cratestack/cbor");
      const codec = await createCborCodec();

      const malformed = new Uint8Array([0x1b]);
      expect(() => codec.decode(malformed)).toThrow();
    });
  },
);

describe.skipIf(!cborNodeIsBuilt)("@cratestack/cbor/node (explicit escape-hatch subpath)", () => {
  // Acceptance criterion: "An explicit escape-hatch subpath exists and is
  // tested, for environments where automatic exports condition resolution
  // doesn't behave as expected." This proves the subpath resolves and
  // behaves identically to the root entry point's Node resolution above,
  // independent of which condition the resolver would have picked
  // automatically.
  it("resolves directly to the native codec and round-trips a value", async () => {
    const { createCborCodec } = await import("@cratestack/cbor/node");
    const codec: CratestackRpcCodec = await createCborCodec();

    expect(codec.contentType).toBe("application/cbor");
    const value = { escape: "hatch", n: 1 };
    const bytes = codec.encode(value) as Uint8Array;
    expect(codec.decode(bytes)).toEqual(value);
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
