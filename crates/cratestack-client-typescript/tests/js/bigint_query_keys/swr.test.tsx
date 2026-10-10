// ADR 0019, SWR layout (`src/swr/`): the design says SWR needs nothing, because
// `stableHash` renders any other primitive with `'' + arg`, so a `bigint` key
// part hashes to its digits. This is the run that checks it: the keys the
// generated hooks build, through SWR's own `unstable_serialize`, and a hook
// over a real SWR cache.
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { SWRConfig, unstable_serialize } from "swr";
import { afterEach, describe, expect, it } from "vitest";
import {
  countRequests,
  recordingFetch,
  type Sent,
  TRANSPORT,
  TWO_53,
  TWO_53_PLUS_ONE,
  wire,
} from "./harness.js";
import { useCounter } from "./src/swr/models/counter.hooks.js";
import { useBalanceQuery, useSearchCountersQuery } from "./src/swr/procedures.hooks.js";
import * as swrRuntime from "./src/swr/runtime.js";
import { swrKeys } from "./src/swr/swr-keys.js";

afterEach(cleanup);

const runtimeFor = (sent: Sent[]) =>
  TRANSPORT === "rpc"
    ? new swrRuntime.CratestackRpcRuntime("http://example.invalid", { fetch: recordingFetch(sent) })
    : new swrRuntime.CratestackRuntime("http://example.invalid", { fetch: recordingFetch(sent) });

const search = (after: bigint) => ({ query: { where: { hitsE8: { gt: after } } } });

describe("swrKeys with a bigint", () => {
  it("serialize, and keep 2^53 and 2^53 + 1 apart", () => {
    const serializers: Array<(value: bigint) => string> = [
      (id) => unstable_serialize(swrKeys.model.Counter.get(id)),
      (id) => unstable_serialize(swrKeys.model.Counter.update(id)),
      (id) => unstable_serialize(swrKeys.procedure.balance({ counterId: id })),
      (id) => unstable_serialize(swrKeys.procedure.searchCounters(search(id))),
    ];
    for (const serialize of serializers) {
      expect(() => serialize(TWO_53)).not.toThrow();
      expect(serialize(TWO_53)).toBe(serialize(2n ** 53n));
      expect(serialize(TWO_53)).not.toBe(serialize(TWO_53_PLUS_ONE));
    }
  });

  it("leave the nullish-argument idiom alone", () => {
    expect(swrKeys.model.Counter.get(null)).toBeNull();
    expect(swrKeys.procedure.balance(undefined)).toBeNull();
  });
});

describe("the SWR hooks with a bigint", () => {
  const isolated = ({ children }: { children: ReactNode }) =>
    createElement(
      SWRConfig,
      { value: { provider: () => new Map(), dedupingInterval: 0 } },
      children,
    );

  it("useCounter caches per bigint id and sends the id as its decimal string", async () => {
    const sent: Sent[] = [];
    const runtime = runtimeFor(sent);
    const first = renderHook(() => useCounter(runtime, TWO_53), { wrapper: isolated });
    await waitFor(() => expect(first.result.current.data).toBeDefined());
    expect(first.result.current.data?.hitsE8).toBe(9007199254740993n);
    expect(wire(sent)).toContain("9007199254740992");

    const next = renderHook(() => useCounter(runtime, TWO_53_PLUS_ONE), { wrapper: isolated });
    await waitFor(() => expect(next.result.current.data).toBeDefined());
    expect(countRequests(sent, "9007199254740993")).toBe(1);
  });

  // One hook at a time: with `@testing-library/react`, a second `renderHook`
  // mounted before the first has resolved never gets its data, bigint or not
  // (reproduced with two plain-string keys), which is a harness limit.
  it("the procedure hooks take a bigint argument and a bigint filter", async () => {
    const sent: Sent[] = [];
    const runtime = runtimeFor(sent);
    const balance = renderHook(() => useBalanceQuery(runtime, { counterId: TWO_53 }), {
      wrapper: isolated,
    });
    await waitFor(() => expect(balance.result.current.data).toBe(7n));
    const found = renderHook(() => useSearchCountersQuery(runtime, search(TWO_53)), {
      wrapper: isolated,
    });
    await waitFor(() => expect(found.result.current.data?.[0]?.hitsE8).toBe(9007199254740993n));
    expect(wire(sent)).toContain('"counterId":"9007199254740992"');
    expect(wire(sent)).toContain('"gt":"9007199254740992"');
  });
});
