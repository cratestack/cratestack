// ADR 0019: a `BigInt` id, filter value or procedure argument is a `bigint`,
// and TanStack Query hashes a key with `JSON.stringify`, which throws on one.
// Run for real against the generated `src/react-query.ts`: the keys, and the
// hooks over a real `QueryClient`.
import { hashKey, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { countRequests, makeClient, TWO_53, TWO_53_PLUS_ONE, wire } from "./harness.js";
import {
  cratestackQueryKeys as keys,
  useBalanceQuery,
  useBumpMutation,
  useCounterListQuery,
  useCounterQuery,
  useLedgerListQuery,
  useSearchCountersQuery,
} from "./src/react-query.js";

afterEach(cleanup);

// `staleTime: Infinity` so a second read of the same key is served from the
// cache: the request count is then a cache-key assertion.
const newQueryClient = () =>
  new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
  });

const wrapperFor =
  (queryClient: QueryClient) =>
  ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);

const search = (after: bigint) => ({
  query: { where: { hitsE8: { gt: after, in: [1n, after] } } },
});

describe("cratestackQueryKeys with a bigint", () => {
  it("control: a key holding a raw bigint does not hash", () => {
    expect(() => hashKey(["/counters", "detail", TWO_53])).toThrow(TypeError);
  });

  it("hashes for a bigint id, a bigint filter and a bigint procedure argument", () => {
    expect(() => hashKey(keys.counterDetail(TWO_53))).not.toThrow();
    expect(() => hashKey(keys.counterList({ query: { limit: 10 } }))).not.toThrow();
    expect(() => hashKey(keys.balanceProcedure({ counterId: TWO_53 }))).not.toThrow();
    expect(() => hashKey(keys.searchCountersProcedure(search(TWO_53)))).not.toThrow();
  });

  it("gives one value one key, and 2^53 and 2^53 + 1 two keys", () => {
    const hashes: Array<(value: bigint) => string> = [
      (id) => hashKey(keys.counterDetail(id)),
      (id) => hashKey(keys.balanceProcedure({ counterId: id })),
      (id) => hashKey(keys.searchCountersProcedure(search(id))),
    ];
    for (const hash of hashes) {
      expect(hash(TWO_53)).toBe(hash(2n ** 53n));
      expect(hash(TWO_53)).not.toBe(hash(TWO_53_PLUS_ONE));
    }
    // A double cannot tell them apart: this is what the canonical string buys.
    expect(Number(TWO_53)).toBe(Number(TWO_53_PLUS_ONE));
  });

  it("stores the canonical decimal string", () => {
    expect(JSON.stringify(keys.counterDetail(-9223372036854775808n))).toContain(
      '"-9223372036854775808"',
    );
    expect(JSON.stringify(keys.searchCountersProcedure(search(TWO_53)))).toContain(
      '"gt":"9007199254740992","in":["1","9007199254740992"]',
    );
  });
});

describe("the hooks, with a bigint", () => {
  it("useCounterQuery holds one cache entry per distinct bigint id", async () => {
    const { client, sent } = makeClient();
    const queryClient = newQueryClient();
    const wrapper = wrapperFor(queryClient);

    const first = renderHook(() => useCounterQuery(client, TWO_53), { wrapper });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    // The response is revived into a bigint; the request carried the string.
    expect(first.result.current.data?.hitsE8).toBe(9007199254740993n);
    expect(wire(sent)).toContain("9007199254740992");

    const same = renderHook(() => useCounterQuery(client, 2n ** 53n), { wrapper });
    await waitFor(() => expect(same.result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryCache().getAll()).toHaveLength(1);
    expect(countRequests(sent, "9007199254740992")).toBe(1);

    const next = renderHook(() => useCounterQuery(client, TWO_53_PLUS_ONE), { wrapper });
    await waitFor(() => expect(next.result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryCache().getAll()).toHaveLength(2);
    expect(countRequests(sent, "9007199254740993")).toBe(1);
  });

  it("useSearchCountersQuery takes a bigint filter and sends it as a string", async () => {
    const { client, sent } = makeClient();
    const queryClient = newQueryClient();
    const wrapper = wrapperFor(queryClient);

    const hook = renderHook(() => useSearchCountersQuery(client, search(TWO_53)), { wrapper });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(hook.result.current.data?.[0]?.hitsE8).toBe(9007199254740993n);
    expect(wire(sent)).toContain('"gt":"9007199254740992"');

    const other = renderHook(() => useSearchCountersQuery(client, search(TWO_53_PLUS_ONE)), {
      wrapper,
    });
    await waitFor(() => expect(other.result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryCache().getAll()).toHaveLength(2);
  });

  it("useBalanceQuery and useBumpMutation take a bigint argument", async () => {
    const { client, sent } = makeClient();
    const wrapper = wrapperFor(newQueryClient());

    const balance = renderHook(() => useBalanceQuery(client, { counterId: TWO_53 }), { wrapper });
    await waitFor(() => expect(balance.result.current.isSuccess).toBe(true));
    expect(balance.result.current.data).toBe(7n);

    const bump = renderHook(() => useBumpMutation(client), { wrapper });
    bump.result.current.mutate({ counterId: TWO_53, byE8: -1n });
    await waitFor(() => expect(bump.result.current.isSuccess).toBe(true));
    expect(wire(sent)).toContain('"counterId":"9007199254740992"');
    expect(wire(sent)).toContain('"byE8":"-1"');
  });

  it("invalidating by cratestackQueryKeys refetches exactly that bigint id", async () => {
    const { client, sent } = makeClient();
    const queryClient = newQueryClient();
    const wrapper = wrapperFor(queryClient);
    const five = renderHook(() => useCounterQuery(client, 5n), { wrapper });
    const six = renderHook(() => useCounterQuery(client, 6n), { wrapper });
    await waitFor(() =>
      expect(five.result.current.isSuccess && six.result.current.isSuccess).toBe(true),
    );
    const before = sent.length;

    await queryClient.invalidateQueries({ queryKey: keys.counterDetail(5n) });
    await waitFor(() => expect(sent.length).toBe(before + 1));
    const refetched = `${sent.at(-1)?.url} ${sent.at(-1)?.text}`;
    expect(refetched).toMatch(/5/);
    expect(refetched).not.toMatch(/6/);
  });

  it("a model whose key is not a bigint still returns revived bigints", async () => {
    const { client } = makeClient();
    const wrapper = wrapperFor(newQueryClient());
    const hook = renderHook(() => useLedgerListQuery(client), { wrapper });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(hook.result.current.data?.items[0]?.amountE8).toBe(9007199254740993n);
  });

  it("useCounterListQuery still keys on a plain query", async () => {
    const { client } = makeClient();
    const queryClient = newQueryClient();
    const hook = renderHook(() => useCounterListQuery(client, { query: { limit: 10 } }), {
      wrapper: wrapperFor(queryClient),
    });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryCache().getAll()).toHaveLength(1);
  });
});
