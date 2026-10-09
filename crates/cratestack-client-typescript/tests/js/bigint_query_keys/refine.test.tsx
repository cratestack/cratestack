// ADR 0019, refine. The design says a `BigInt` id "stays the wire string, which
// is a BaseKey" and "the provider does not revive it". It does hand refine the
// generated client's REVIVED record, so `record.id` is a `bigint`, which is not
// a `BaseKey`. This run is the check that matters: real `@refinedev/core`
// hooks over the repository's own `@cratestack/refine` source (copied in as
// `./refine-pkg`) and the generated client, with bigint ids in the records,
// must not throw anywhere in refine's query keys, cache or mutations.
import { Refine, useDelete, useList, useMany, useOne, useUpdate } from "@refinedev/core";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { createElement, useEffect } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { makeClient, TRANSPORT, TWO_53 } from "./harness.js";
import {
  createCratestackDataProvider,
  createCratestackRpcDataProvider,
} from "./refine-pkg/index.js";

afterEach(cleanup);

// What the generated `src/refine.ts` (`--refine`) returns for this schema,
// written out because that file imports its type from the published
// `@cratestack/refine`, which this run replaces with the source under test.
function dataProvider() {
  const { client, sent } = makeClient();
  const resources = { counters: { api: client.counters, primaryKey: "id", paged: false } };
  const provider =
    TRANSPORT === "rpc"
      ? createCratestackRpcDataProvider(resources as never)
      : createCratestackDataProvider(resources as never);
  return { provider, sent };
}

function Screen() {
  const list = useList({ resource: "counters" });
  const one = useOne({ resource: "counters", id: TWO_53.toString() });
  const many = useMany({ resource: "counters", ids: [TWO_53.toString()] });
  const remove = useDelete();
  const update = useUpdate();
  const first = list.result?.data?.[0];
  useEffect(() => {
    if (first) {
      // `first.id` is the record's own bigint id, as a list row's action
      // button would pass it.
      update.mutate({ resource: "counters", id: first.id as never, values: { label: "x" } });
      remove.mutate({ resource: "counters", id: first.id as never });
    }
  }, [first?.id]);
  return createElement(
    "div",
    null,
    [
      typeof first?.id,
      one.result?.hitsE8 !== undefined ? typeof one.result.hitsE8 : "-",
      String(many.result?.data?.length),
      update.mutation.status,
      remove.mutation.status,
    ].join("|"),
  );
}

describe("refine over a generated client with bigint ids", () => {
  it("lists, reads, and mutates without throwing", async () => {
    const { provider, sent } = dataProvider();
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const view = render(
      createElement(
        Refine,
        {
          dataProvider: provider,
          resources: [{ name: "counters" }],
          options: { disableTelemetry: true },
        },
        createElement(Screen),
      ),
    );
    // list row id is a bigint, the read hook revived hitsE8, getMany found one
    // row, and both mutations succeeded.
    await waitFor(() => expect(view.container.textContent).toBe("bigint|bigint|1|success|success"));
    await act(async () => {});
    expect(errors.mock.calls.map((call) => String(call[0]))).toEqual([]);
    errors.mockRestore();
    expect(sent.length).toBeGreaterThan(3);
  });
});
