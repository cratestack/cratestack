// `compactKeys: true` against a `@cratestack/cbor` that has no
// `createCompactKeys` (every release before the compact keys one): silently
// named, never an error.
import { describe, expect, it, vi } from "vitest";
import { createCborCodec } from "@cratestack/cbor";
import * as plain from "./src/runtime.js";
import * as swr from "./src/swr/runtime.js";

vi.mock("@cratestack/cbor", async (importOriginal) => {
  const real = await importOriginal<Record<string, unknown>>();
  return { ...real, createCompactKeys: undefined };
});

describe.each([
  ["default layout", plain],
  ["--swr layout", swr],
] as const)("%s", (_name, mod) => {
  it("stays named when the binding lacks createCompactKeys", async () => {
    const codec = await createCborCodec();
    const seen: Headers[] = [];
    const fetchFn: typeof fetch = async (_input, init) => {
      seen.push(new Headers(init?.headers));
      return new Response(codec.encode("hi") as Uint8Array, {
        headers: { "Content-Type": "application/cbor" },
      });
    };
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    expect(await runtime.call("procedure.echoName", { name: "a" })).toBe("hi");
    expect(await runtime.call("procedure.echoName", { name: "b" })).toBe("hi");
    for (const headers of seen) {
      expect(headers.get("accept")).toBe("application/cbor");
      expect(headers.has("cratestack-contract")).toBe(false);
    }
  });
});
