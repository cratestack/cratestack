// ADR 0019: a `BigInt` id or procedure argument is a `bigint` query argument
// of the generated RTK Query endpoints. RTK Query builds the cache key from the
// argument with its default `serializeQueryArgs`, which throws on a `bigint`
// before `@reduxjs/toolkit` 2.2.4, so `tests/bigint_query_keys.rs` runs this
// file twice: on the newest 2.x, and on the exact floor the package declares.
import { configureStore, isPlain } from "@reduxjs/toolkit";
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { Provider } from "react-redux";
import { afterEach, describe, expect, it, vi } from "vitest";
import { countRequests, makeClient, TWO_53, TWO_53_PLUS_ONE, wire } from "./harness.js";
import { createCratestackRtkApi } from "./src/rtk-api.js";

afterEach(cleanup);

// The store option the generated `rtk-api.ts` documents: RTK's development-only
// serializability check treats a `bigint` as non-serializable and, without
// this, logs it for every `originalArgs` and every revived `bigint` field.
const allowBigInt = {
  isSerializable: (value: unknown) => typeof value === "bigint" || isPlain(value),
};

function setup() {
  const { client, sent } = makeClient();
  const api = createCratestackRtkApi(client);
  const store = configureStore({
    reducer: { [api.reducerPath]: api.reducer },
    middleware: (getDefault) =>
      getDefault({ serializableCheck: allowBigInt }).concat(api.middleware),
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(Provider, { store, children });
  const cacheKeys = () => Object.keys(store.getState()[api.reducerPath].queries);
  return { api, sent, store, wrapper, cacheKeys };
}

const search = (after: bigint) => ({ query: { where: { hitsE8: { gt: after } } } });

describe("RTK Query endpoints with a bigint argument", () => {
  it("getCounter holds one cache entry per distinct bigint id", async () => {
    const { api, sent, wrapper, cacheKeys } = setup();

    const first = renderHook(() => api.useGetCounterQuery(TWO_53), { wrapper });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    expect(first.result.current.data?.hitsE8).toBe(9007199254740993n);
    expect(wire(sent)).toContain("9007199254740992");

    const same = renderHook(() => api.useGetCounterQuery(2n ** 53n), { wrapper });
    await waitFor(() => expect(same.result.current.isSuccess).toBe(true));
    expect(cacheKeys()).toHaveLength(1);
    expect(countRequests(sent, "9007199254740992")).toBe(1);

    const next = renderHook(() => api.useGetCounterQuery(TWO_53_PLUS_ONE), { wrapper });
    await waitFor(() => expect(next.result.current.isSuccess).toBe(true));
    expect(cacheKeys()).toHaveLength(2);
    expect(cacheKeys().join(" ")).toContain('{"$bigint":"9007199254740993"}');
  });

  it("a bigint filter and a bigint procedure argument are cache-key parts", async () => {
    const { api, sent, wrapper, cacheKeys } = setup();

    const found = renderHook(() => api.useSearchCountersQuery(search(TWO_53)), { wrapper });
    const balance = renderHook(() => api.useBalanceQuery({ counterId: TWO_53 }), { wrapper });
    await waitFor(() =>
      expect(found.result.current.isSuccess && balance.result.current.isSuccess).toBe(true),
    );
    expect(found.result.current.data?.[0]?.hitsE8).toBe(9007199254740993n);
    expect(balance.result.current.data).toBe(7n);
    expect(wire(sent)).toContain('"gt":"9007199254740992"');

    const other = renderHook(() => api.useSearchCountersQuery(search(TWO_53_PLUS_ONE)), {
      wrapper,
    });
    await waitFor(() => expect(other.result.current.isSuccess).toBe(true));
    expect(cacheKeys()).toHaveLength(3);
  });

  it("a bigint-keyed update refetches that id and not a neighbour", async () => {
    const { api, sent, store, wrapper } = setup();
    const five = renderHook(() => api.useGetCounterQuery(5n), { wrapper });
    const six = renderHook(() => api.useGetCounterQuery(6n), { wrapper });
    await waitFor(() =>
      expect(five.result.current.isSuccess && six.result.current.isSuccess).toBe(true),
    );
    const before = sent.length;

    // The tag a bigint id provides is its decimal string, the same one the
    // update invalidates, so exactly the id-5 entry refetches.
    await store.dispatch(api.endpoints.updateCounter.initiate({ id: 5n, input: { label: "x" } }));
    const refetches = () =>
      sent
        .slice(before)
        .filter((s) => !/PATCH|Counter\.update/.test(`${s.method} ${s.url}`))
        .map((s) => `${s.url} ${s.text}`);
    await waitFor(() => expect(refetches().length).toBeGreaterThan(0));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(refetches()).toHaveLength(1);
    expect(refetches()[0]).toMatch(/5/);
    expect(refetches()[0]).not.toMatch(/6/);
  });

  it("the documented serializableCheck option keeps the console clean", async () => {
    // Without it, RTK (2.13.0, RAN) logs "A non-serializable value was
    // detected" for `...originalArgs` and for each revived `bigint` field.
    const complaints = vi.spyOn(console, "error").mockImplementation(() => {});
    const { api, wrapper } = setup();
    const hook = renderHook(() => api.useGetCounterQuery(TWO_53), { wrapper });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    const messages = complaints.mock.calls.map((call) => String(call[0]));
    complaints.mockRestore();
    expect(messages).toEqual([]);
  });
});
