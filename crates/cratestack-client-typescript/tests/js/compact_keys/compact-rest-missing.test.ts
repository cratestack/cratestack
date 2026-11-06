// `compactKeys: true` where `@cratestack/cbor` is absent, or predates
// `createCompactKeys`: silently named, never an error.
import { describe, expect, it, vi } from "vitest";
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
    const seen: Headers[] = [];
    const fetchFn: typeof fetch = async (_input, init) => {
      seen.push(new Headers(init?.headers));
      return new Response("{}", { headers: { "Content-Type": "application/json" } });
    };
    const runtime = new mod.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.get("/widgets/1");
    await runtime.post("/widgets", { name: "a" });
    for (const headers of seen) {
      expect(headers.get("accept")).toBe("application/json");
      expect(headers.has("cratestack-contract")).toBe(false);
    }
  });
});
