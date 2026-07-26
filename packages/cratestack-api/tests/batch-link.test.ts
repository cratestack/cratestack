import { describe, expect, it, vi } from "vitest";
import { createBatchLink } from "../src/batch-link.js";
import type { RpcRequest, RpcResponseFrame } from "../src/types.js";
import { FakeRuntime } from "./harness.js";

/** Fake `/rpc/batch` server: decodes the request array, resolves each
 *  frame via `resolver(op, input)`, returns frames in request order —
 *  the same order-preserving contract the real server guarantees
 *  (docs/design/rpc-transport.md §3.2). */
function fakeBatchFetch(resolver: (op: string, input: unknown) => unknown) {
  return vi.fn(async (_url: string, init: RequestInit) => {
    const requests = JSON.parse(init.body as string) as RpcRequest[];
    const frames: RpcResponseFrame[] = requests.map((request) => ({
      id: request.id,
      output: resolver(request.op, request.input),
    }));
    return new Response(JSON.stringify(frames), { status: 200 });
  });
}

describe("createBatchLink", () => {
  it("collapses concurrent unary calls issued in the same tick into one /rpc/batch request", async () => {
    const fetchMock = fakeBatchFetch((op, input) => ({ op, input }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const results = await Promise.all([
      runtime.call("model.Widget.get", { id: 1 }),
      runtime.call("model.Widget.get", { id: 2 }),
      runtime.call("model.Widget.get", { id: 3 }),
    ]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://example.test/rpc/batch");
    expect(results).toEqual([
      { op: "model.Widget.get", input: { id: 1 } },
      { op: "model.Widget.get", input: { id: 2 } },
      { op: "model.Widget.get", input: { id: 3 } },
    ]);
  });

  it("fans results back out to the right caller by request order", async () => {
    const fetchMock = fakeBatchFetch((op, input) => `${op}:${JSON.stringify(input)}`);
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const [a, b] = await Promise.all([
      runtime.call<string>("model.Widget.get", { id: "a" }),
      runtime.call<string>("model.Widget.get", { id: "b" }),
    ]);

    expect(a).toBe('model.Widget.get:{"id":"a"}');
    expect(b).toBe('model.Widget.get:{"id":"b"}');
  });

  it("dedupes concurrent calls that share an idempotencyKey by default", async () => {
    let executions = 0;
    const fetchMock = fakeBatchFetch(() => {
      executions += 1;
      return { executions };
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const [a, b] = await Promise.all([
      runtime.call<{ executions: number }>(
        "model.Order.create",
        { total: 10 },
        { idempotencyKey: "order-1" },
      ),
      runtime.call<{ executions: number }>(
        "model.Order.create",
        { total: 10 },
        { idempotencyKey: "order-1" },
      ),
    ]);

    const [url, init] = fetchMock.mock.calls[0]!;
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const requests = JSON.parse((init as RequestInit).body as string) as RpcRequest[];
    expect(requests).toHaveLength(1);
    expect(a).toEqual({ executions: 1 });
    expect(b).toEqual({ executions: 1 });
    void url;
  });

  it("does NOT dedupe identical calls with no idempotencyKey (unsafe to auto-collapse mutations)", async () => {
    const fetchMock = fakeBatchFetch((_op, input) => input);
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    await Promise.all([
      runtime.call("model.Order.create", { total: 10 }),
      runtime.call("model.Order.create", { total: 10 }),
    ]);

    const [, init] = fetchMock.mock.calls[0]!;
    const requests = JSON.parse((init as RequestInit).body as string) as RpcRequest[];
    expect(requests).toHaveLength(2);
  });

  it("rejects only the queued entries whose response frame id is missing", async () => {
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify([{ id: 0, output: "only one" }]), { status: 200 }),
    );
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    await expect(
      Promise.all([
        runtime.call("model.Widget.get", { id: 1 }),
        runtime.call("model.Widget.get", { id: 2 }),
      ]),
    ).rejects.toThrow(/missing frame id/);
  });

  it("correlates results by frame id, not array position — order-independent fan-out", async () => {
    // Mirrors the repo's own Rust client-side batch debouncer
    // (examples/rpc-batch-debounce), which matches responders by `id`
    // via a map rather than trusting response array order. The server
    // contract (docs/design/rpc-transport.md §3.2) guarantees order,
    // but this proves the link doesn't silently depend on it.
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      const requests = JSON.parse(init.body as string) as RpcRequest[];
      const frames: RpcResponseFrame[] = requests.map((request) => ({
        id: request.id,
        output: request.input,
      }));
      // Reverse the response order relative to the request order.
      return new Response(JSON.stringify(frames.reverse()), { status: 200 });
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const [first, second] = await Promise.all([
      runtime.call<{ marker: string }>("model.Widget.get", { marker: "first" }),
      runtime.call<{ marker: string }>("model.Widget.get", { marker: "second" }),
    ]);

    expect(first).toEqual({ marker: "first" });
    expect(second).toEqual({ marker: "second" });
  });

  it("passes an explicit runtime.batch() call straight through without re-batching it", async () => {
    const fetchMock = fakeBatchFetch((op, input) => ({ op, input }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const result = await runtime.batch([{ id: 0, op: "procedure.echo", input: { hi: true } }]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(result).toEqual([{ id: 0, output: { op: "procedure.echo", input: { hi: true } } }]);
  });
});
