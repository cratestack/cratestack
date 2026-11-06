// `compactKeys: true` where `@cratestack/cbor` cannot be imported at all (an
// optional peer an app never installed): the dynamic import rejects and the
// wire stays named.
import { describe, expect, it, vi } from "vitest";
import * as plain from "./src/runtime.js";

vi.mock("@cratestack/cbor", () => {
  throw new Error("Cannot find package '@cratestack/cbor'");
});

describe("default layout", () => {
  it("stays named when @cratestack/cbor cannot be imported", async () => {
    const seen: Headers[] = [];
    const fetchFn: typeof fetch = async (_input, init) => {
      seen.push(new Headers(init?.headers));
      return new Response("{}", { headers: { "Content-Type": "application/json" } });
    };
    const runtime = new plain.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.get("/widgets/1");
    expect(seen[0]?.get("accept")).toBe("application/json");
    expect(seen[0]?.has("cratestack-contract")).toBe(false);
  });
});
