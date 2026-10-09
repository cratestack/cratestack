import { hashKey, QueryClient } from "@tanstack/query-core";
import { describe, expect, it, vi } from "vitest";
import {
  isRpcErrorCode,
  type RpcCaller,
  rpcMutationOptions,
  rpcQueryKey,
  rpcQueryOptions,
} from "../src/index.js";

function fakeClient(call: RpcCaller["call"]): RpcCaller {
  return { call };
}

describe("rpcQueryKey", () => {
  it("includes input when provided", () => {
    expect(rpcQueryKey("model.Widget.get", { id: 1 })).toEqual(["model.Widget.get", { id: 1 }]);
  });

  it("omits input entirely when undefined, rather than a trailing undefined slot", () => {
    expect(rpcQueryKey("procedure.ping")).toEqual(["procedure.ping"]);
  });
});

// ADR 0019: a `BigInt` field of a generated client is a `bigint`, and
// TanStack Query hashes a key with `JSON.stringify`, which throws on one.
describe("rpcQueryKey with a bigint", () => {
  const OP = "model.Counter.get";
  const TWO_53 = 2n ** 53n;

  it("control: the raw bigint key does not hash, which is why the key is encoded", () => {
    expect(() => hashKey([OP, { id: TWO_53 }])).toThrow(TypeError);
    expect(() => hashKey(rpcQueryKey(OP, { id: TWO_53 }))).not.toThrow();
  });

  it("stores the canonical decimal string, at any depth", () => {
    expect(rpcQueryKey(OP, { id: TWO_53 })).toEqual([OP, { id: "9007199254740992" }]);
    expect(
      rpcQueryKey("procedure.search", {
        where: { hitsE8: { gt: -9223372036854775808n, in: [1n, 2n] } },
        page: [{ after: 9223372036854775807n }],
      }),
    ).toEqual([
      "procedure.search",
      {
        where: { hitsE8: { gt: "-9223372036854775808", in: ["1", "2"] } },
        page: [{ after: "9223372036854775807" }],
      },
    ]);
    expect(rpcQueryKey(OP, 5n)).toEqual([OP, "5"]);
  });

  it("gives the same value the same hash and 2^53 and 2^53 + 1 different ones", () => {
    const hash = (id: bigint) => hashKey(rpcQueryKey(OP, { id }));
    expect(hash(TWO_53)).toBe(hash(2n ** 53n));
    expect(hash(TWO_53)).not.toBe(hash(TWO_53 + 1n));
    // A double cannot tell these two apart, which is the point of the type.
    expect(Number(TWO_53)).toBe(Number(TWO_53 + 1n));
  });

  it("leaves every other value alone, class instances by identity", () => {
    const when = new Date(0);
    const bytes = new Uint8Array([1, 2]);
    const key = rpcQueryKey(OP, { id: 1, when, bytes, none: null, nested: { flag: true } });
    expect(key).toEqual([OP, { id: 1, when, bytes, none: null, nested: { flag: true } }]);
    const input = key[1] as { when: Date; bytes: Uint8Array };
    expect(input.when).toBe(when);
    expect(input.bytes).toBe(bytes);
  });

  it("does not mutate the caller's input", () => {
    const input = { id: TWO_53, ids: [1n] };
    rpcQueryKey(OP, input);
    expect(input).toEqual({ id: TWO_53, ids: [1n] });
  });
});

describe("rpcQueryOptions", () => {
  it("builds a queryKey/queryFn pair that calls through to the client", async () => {
    const call = vi.fn(async (opId: string, input: unknown) => ({ opId, input }));
    const client = fakeClient(call as unknown as RpcCaller["call"]);
    const options = rpcQueryOptions(client, "model.Widget.get", { id: 1 });

    expect(options.queryKey).toEqual(["model.Widget.get", { id: 1 }]);
    const controller = new AbortController();
    const result = await options.queryFn({ signal: controller.signal });

    expect(result).toEqual({ opId: "model.Widget.get", input: { id: 1 } });
    expect(call).toHaveBeenCalledWith("model.Widget.get", { id: 1 }, { signal: controller.signal });
  });
});

describe("rpcQueryOptions with a bigint input", () => {
  it("fetches through a real QueryClient, one cache entry per distinct bigint", async () => {
    const call = vi.fn(async (_opId: string, input: unknown) => input);
    const client = fakeClient(call as unknown as RpcCaller["call"]);
    const queryClient = new QueryClient();
    const TWO_53 = 2n ** 53n;

    // `staleTime: Infinity` so a second fetch of the same key is served from
    // the cache, which is what makes the call count a cache-key assertion.
    const fetch = (id: bigint) =>
      queryClient.fetchQuery({
        ...rpcQueryOptions<{ id: bigint }, { id: bigint }>(client, "model.Counter.get", { id }),
        staleTime: Number.POSITIVE_INFINITY,
      });

    const first = await fetch(TWO_53);
    // The key is the encoded form; the call still gets the real bigint, which
    // the generated runtime turns into its wire string.
    expect(first).toEqual({ id: TWO_53 });
    expect(call).toHaveBeenCalledWith(
      "model.Counter.get",
      { id: TWO_53 },
      { signal: expect.any(AbortSignal) },
    );

    await fetch(2n ** 53n);
    expect(call).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryCache().getAll()).toHaveLength(1);

    await fetch(TWO_53 + 1n);
    expect(call).toHaveBeenCalledTimes(2);
    expect(queryClient.getQueryCache().getAll()).toHaveLength(2);
  });

  it("invalidates by a key built with rpcQueryKey", async () => {
    const call = vi.fn(async (_opId: string, input: unknown) => input);
    const client = fakeClient(call as unknown as RpcCaller["call"]);
    const queryClient = new QueryClient();
    const options = rpcQueryOptions(client, "model.Counter.get", { id: 5n });
    await queryClient.fetchQuery(options);

    await queryClient.invalidateQueries({ queryKey: rpcQueryKey("model.Counter.get", { id: 5n }) });
    expect(queryClient.getQueryState(options.queryKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(rpcQueryKey("model.Counter.get", { id: 6n }))).toBeUndefined();
  });
});

describe("rpcMutationOptions", () => {
  it("builds a mutationKey/mutationFn pair that takes input at call time", async () => {
    const call = vi.fn(async (opId: string, input: unknown) => ({ opId, input }));
    const client = fakeClient(call as unknown as RpcCaller["call"]);
    const options = rpcMutationOptions(client, "model.Order.create");

    expect(options.mutationKey).toEqual(["model.Order.create"]);
    const result = await options.mutationFn({ total: 10 });

    expect(result).toEqual({ opId: "model.Order.create", input: { total: 10 } });
  });
});

describe("isRpcErrorCode", () => {
  it("matches a structurally-shaped RpcErrorBody by code", () => {
    expect(isRpcErrorCode({ code: "not_found", message: "nope" }, "not_found")).toBe(true);
    expect(isRpcErrorCode({ code: "conflict", message: "nope" }, "not_found")).toBe(false);
  });

  it("is false for non-object errors", () => {
    expect(isRpcErrorCode(new Error("boom"), "not_found")).toBe(false);
    expect(isRpcErrorCode(null, "not_found")).toBe(false);
  });
});
