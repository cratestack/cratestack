import type { RpcRequest, RpcResponseFrame } from "@cratestack/ts-types";
import { FakeRuntime } from "@cratestack/ts-types/test-harness";
import { describe, expect, it, vi } from "vitest";
import { createBatchLink } from "../src/index.js";

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

// Issue #273: calls with a divergent transport envelope (headers,
// fetchFn, codec) must never have that envelope silently discarded in
// favor of whichever call happened to be queued first.
describe("createBatchLink partitioning (#273)", () => {
  it("issues one /rpc/batch request per distinct set of headers, each carrying its own headers verbatim", async () => {
    const seenAuth: (string | null)[] = [];
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      seenAuth.push(new Headers(init.headers).get("authorization"));
      const requests = JSON.parse(init.body as string) as RpcRequest[];
      const frames: RpcResponseFrame[] = requests.map((r) => ({ id: r.id, output: r.input }));
      return new Response(JSON.stringify(frames), { status: 200 });
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const [a, b] = await Promise.all([
      runtime.call(
        "model.Widget.get",
        { who: "tenant-a" },
        { headers: { authorization: "Bearer a" } },
      ),
      runtime.call(
        "model.Widget.get",
        { who: "tenant-b" },
        { headers: { authorization: "Bearer b" } },
      ),
    ]);

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(a).toEqual({ who: "tenant-a" });
    expect(b).toEqual({ who: "tenant-b" });
    expect(seenAuth.sort()).toEqual(["Bearer a", "Bearer b"]);
  });

  it("still collapses calls that share identical headers into one request", async () => {
    const fetchMock = fakeBatchFetch((op, input) => ({ op, input }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    await Promise.all([
      runtime.call("model.Widget.get", { id: 1 }, { headers: { authorization: "Bearer shared" } }),
      runtime.call("model.Widget.get", { id: 2 }, { headers: { authorization: "Bearer shared" } }),
    ]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [, init] = fetchMock.mock.calls[0]!;
    const requests = JSON.parse((init as RequestInit).body as string) as RpcRequest[];
    expect(requests).toHaveLength(2);
  });

  it("does NOT partition on Idempotency-Key — it's carried per-frame, not per-request", async () => {
    // Each call sets a DIFFERENT Idempotency-Key header (mirroring what
    // the real generated runtime does for every idempotencyKey-bearing
    // call), but that alone must not fragment the batch — the header is
    // frame-level (RpcRequest.idem), not part of the request envelope.
    const fetchMock = fakeBatchFetch((op, input) => ({ op, input }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    await Promise.all([
      runtime.call("model.Order.create", { total: 1 }, { idempotencyKey: "order-a" }),
      runtime.call("model.Order.create", { total: 2 }, { idempotencyKey: "order-b" }),
    ]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [, init] = fetchMock.mock.calls[0]!;
    const requests = JSON.parse((init as RequestInit).body as string) as RpcRequest[];
    expect(requests.map((r) => r.idem).sort()).toEqual(["order-a", "order-b"]);
  });

  it("issues one request per distinct fetchFn — never merges calls that shouldn't share a transport", async () => {
    const fetchA = fakeBatchFetch((op, input) => ({ via: "a", op, input }));
    const fetchB = fakeBatchFetch((op, input) => ({ via: "b", op, input }));
    const link = createBatchLink();
    const runtimeA = new FakeRuntime(fetchA as unknown as typeof fetch, [link]);
    const runtimeB = new FakeRuntime(fetchB as unknown as typeof fetch, [link]);

    const [a, b] = await Promise.all([
      runtimeA.call<{ via: string }>("procedure.echo", { id: 1 }),
      runtimeB.call<{ via: string }>("procedure.echo", { id: 2 }),
    ]);

    expect(fetchA).toHaveBeenCalledTimes(1);
    expect(fetchB).toHaveBeenCalledTimes(1);
    expect(a.via).toBe("a");
    expect(b.via).toBe("b");
  });

  it("scopes maxBatchSize to each partition independently, not to the whole flush", async () => {
    const fetchMock = fakeBatchFetch((op, input) => ({ op, input }));
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [
      createBatchLink({ maxBatchSize: 2 }),
    ]);

    await Promise.all([
      runtime.call("model.Widget.get", { id: 1 }, { headers: { authorization: "Bearer a" } }),
      runtime.call("model.Widget.get", { id: 2 }, { headers: { authorization: "Bearer a" } }),
      runtime.call("model.Widget.get", { id: 3 }, { headers: { authorization: "Bearer a" } }),
      runtime.call("model.Widget.get", { id: 4 }, { headers: { authorization: "Bearer b" } }),
    ]);

    // Partition "a" (3 calls) chunks into 2+1 under maxBatchSize 2;
    // partition "b" (1 call) is its own single request — 3 total, not
    // a single global chunk of 2 that would straddle both partitions.
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("a failure in one partition rejects only that partition's callers", async () => {
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      const auth = new Headers(init.headers).get("authorization");
      if (auth === "Bearer bad") {
        throw new Error("network down for tenant-bad");
      }
      const requests = JSON.parse(init.body as string) as RpcRequest[];
      const frames: RpcResponseFrame[] = requests.map((r) => ({ id: r.id, output: r.input }));
      return new Response(JSON.stringify(frames), { status: 200 });
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [createBatchLink()]);

    const [good, bad] = await Promise.allSettled([
      runtime.call("model.Widget.get", { id: 1 }, { headers: { authorization: "Bearer good" } }),
      runtime.call("model.Widget.get", { id: 2 }, { headers: { authorization: "Bearer bad" } }),
    ]);

    expect(good.status).toBe("fulfilled");
    expect(bad.status).toBe("rejected");
  });

  it("createBatchLink({ headers }) sets the aggregate request's baseline headers", async () => {
    const fetchMock = vi.fn(async (_url: string, init: RequestInit) => {
      expect(new Headers(init.headers).get("x-batch-source")).toBe("link-default");
      const requests = JSON.parse(init.body as string) as RpcRequest[];
      const frames: RpcResponseFrame[] = requests.map((r) => ({ id: r.id, output: r.input }));
      return new Response(JSON.stringify(frames), { status: 200 });
    });
    const runtime = new FakeRuntime(fetchMock as unknown as typeof fetch, [
      createBatchLink({ headers: { "x-batch-source": "link-default" } }),
    ]);

    await runtime.call("model.Widget.get", { id: 1 });

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
