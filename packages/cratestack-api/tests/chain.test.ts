// Behavioral proof for the RpcLink chain mechanism itself (issue #182).
// `crates/cratestack-client-typescript`'s own test suite only asserts on
// generated *text* (no JS execution harness there) — these properties
// (ordering, abort propagation, no-op equivalence) need real execution,
// so they're proven here against `FakeRuntime`, which mirrors the
// generated chain-construction logic exactly. See the implementation
// plan for why this split is deliberate.
import { describe, expect, it, vi } from "vitest";
import type { RpcLink } from "../src/types.js";
import { FakeRuntime } from "./harness.js";

function okResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200 });
}

describe("RpcLink chain", () => {
  it("is a true no-op when no links are declared — request is byte-identical", async () => {
    const fetchMock = vi.fn(async () => okResponse({ ok: true }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, []);

    await runtime.call("model.Widget.get", { id: 1 });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://example.test/rpc/model.Widget.get");
    expect(init.method).toBe("POST");
    expect(init.body).toBe(JSON.stringify({ id: 1 }));
  });

  it("runs two independent links in declared order without clobbering each other", async () => {
    const order: string[] = [];
    const linkA: RpcLink = async (request, next) => {
      order.push("a:before");
      const result = await next(request);
      order.push("a:after");
      return result;
    };
    const linkB: RpcLink = async (request, next) => {
      order.push("b:before");
      const result = await next(request);
      order.push("b:after");
      return result;
    };
    const fetchMock = vi.fn(async () => {
      order.push("fetch");
      return okResponse({ ok: true });
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [linkA, linkB]);

    await runtime.call("procedure.echo", null);

    expect(order).toEqual(["a:before", "b:before", "fetch", "b:after", "a:after"]);
  });

  it("propagates the per-call AbortSignal through to the terminal fetch", async () => {
    const controller = new AbortController();
    let receivedSignal: AbortSignal | null | undefined;
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      receivedSignal = init.signal as AbortSignal | null;
      return okResponse({ ok: true });
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, []);

    await runtime.call("procedure.echo", null, { signal: controller.signal });

    expect(receivedSignal).toBe(controller.signal);
  });

  it("lets a link short-circuit the chain entirely (never calls next)", async () => {
    const shortCircuit: RpcLink = async () => ({ response: okResponse({ intercepted: true }) });
    const fetchMock = vi.fn(async () => okResponse({ real: true }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [shortCircuit]);

    const result = await runtime.call<{ intercepted: boolean }>("procedure.echo", null);

    expect(result).toEqual({ intercepted: true });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
