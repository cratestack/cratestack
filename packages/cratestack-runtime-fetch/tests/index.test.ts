import { describe, expect, it, vi } from "vitest";
import { createFetchRuntime } from "../src/index.js";

describe("createFetchRuntime", () => {
  it("is a byte-identical pass-through to the global fetch with no options", () => {
    expect(createFetchRuntime()).toBe(fetch);
  });

  it("delegates to a custom fetch when no timeout is set", async () => {
    const baseFetch = vi.fn(async () => new Response(null, { status: 204 }));
    const runtime = createFetchRuntime({ fetch: baseFetch as unknown as typeof fetch });

    await runtime("https://example.test", { method: "GET" });

    expect(baseFetch).toHaveBeenCalledWith("https://example.test", { method: "GET" });
  });

  it("aborts the call once timeoutMs elapses", async () => {
    const baseFetch = vi.fn(
      (_input: RequestInfo | URL, init?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () => reject(init.signal!.reason));
        }),
    );
    const runtime = createFetchRuntime({
      fetch: baseFetch as unknown as typeof fetch,
      timeoutMs: 5,
    });

    await expect(runtime("https://example.test")).rejects.toBeInstanceOf(DOMException);
  });

  it("aborts when the caller's own signal fires, even with a timeout configured", async () => {
    const controller = new AbortController();
    const baseFetch = vi.fn(
      (_input: RequestInfo | URL, init?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () => reject(init.signal!.reason));
        }),
    );
    const runtime = createFetchRuntime({
      fetch: baseFetch as unknown as typeof fetch,
      timeoutMs: 60_000,
    });

    const call = runtime("https://example.test", { signal: controller.signal });
    controller.abort(new Error("caller cancelled"));

    await expect(call).rejects.toThrow("caller cancelled");
  });
});
