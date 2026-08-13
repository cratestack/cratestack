import type { CrudFilters } from "@refinedev/core";
import { describe, expect, it } from "vitest";
import { toRpcQueryFilters, toRpcSortQuery } from "../src/rpc-filters.js";
import { createCratestackRpcDataProvider } from "../src/rpc-provider.js";
import { createRpcTestClient, WIDGET_RPC_SCHEMA } from "./support/rpc-client.js";

describe("toRpcQueryFilters", () => {
  it("maps eq to a bare field key, same as REST's toQueryFilters", () => {
    expect(toRpcQueryFilters([{ field: "name", operator: "eq", value: "gizmo" }])).toEqual([
      { key: "name", value: "gizmo" },
    ]);
  });

  it("maps ne to field__ne", () => {
    expect(toRpcQueryFilters([{ field: "name", operator: "ne", value: "gizmo" }])).toEqual([
      { key: "name__ne", value: "gizmo" },
    ]);
  });

  it("maps in to field__in as a comma-joined list", () => {
    expect(toRpcQueryFilters([{ field: "id", operator: "in", value: [1, 2, 3] }])).toEqual([
      { key: "id__in", value: "1,2,3" },
    ]);
  });

  it("maps null/nnull to field__isNull true/false", () => {
    expect(toRpcQueryFilters([{ field: "weight", operator: "null", value: true }])).toEqual([
      { key: "weight__isNull", value: "true" },
    ]);
    expect(toRpcQueryFilters([{ field: "weight", operator: "nnull", value: true }])).toEqual([
      { key: "weight__isNull", value: "false" },
    ]);
  });

  it("throws on an operator with no cratestack equivalent instead of dropping the filter", () => {
    const unsupported: CrudFilters = [{ field: "name", operator: "endswith", value: "get" }];
    expect(() => toRpcQueryFilters(unsupported)).toThrow(/no cratestack equivalent/);
  });

  it("throws on a refine conditional filter group (or/and), not just unmapped field operators", () => {
    const group: CrudFilters = [
      { operator: "or", value: [{ field: "name", operator: "eq", value: "a" }] },
    ];
    expect(() => toRpcQueryFilters(group)).toThrow(/filter groups have no cratestack equivalent/);
  });
});

describe("toRpcSortQuery", () => {
  it("joins fields with commas, prefixing descending fields with -, unlike REST's array", () => {
    expect(
      toRpcSortQuery([
        { field: "createdAt", order: "desc" },
        { field: "id", order: "asc" },
      ]),
    ).toBe("-createdAt,id");
  });

  it("returns undefined for no sorters", () => {
    expect(toRpcSortQuery([])).toBeUndefined();
  });
});

describe("getList filter mapping against a real generated RPC client", () => {
  it("threads a mapped filter through as a real RpcListPredicate and gets back the filtered rows", async () => {
    const { server, client } = createRpcTestClient([WIDGET_RPC_SCHEMA]);
    server.seed("Widget", { id: 1, name: "gizmo", weight: 3 });
    server.seed("Widget", { id: 2, name: "gadget", weight: 5 });

    const provider = createCratestackRpcDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    const result = await provider.getList({
      resource: "widgets",
      filters: [{ field: "weight", operator: "gte", value: 4 }],
    });

    const listCall = server.requests.find((r) => r.opId === "model.Widget.list");
    expect(listCall?.body).toMatchObject({ filters: [{ key: "weight__gte", value: "4" }] });
    expect(result.data).toEqual([{ id: 2, name: "gadget", weight: 5 }]);
  });

  it("rejects an unsupported operator before ever calling the real client", async () => {
    const { client } = createRpcTestClient([WIDGET_RPC_SCHEMA]);
    const provider = createCratestackRpcDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    await expect(
      provider.getList({
        resource: "widgets",
        filters: [{ field: "name", operator: "endswith", value: "o" }],
      }),
    ).rejects.toThrow(/no cratestack equivalent/);
  });
});
