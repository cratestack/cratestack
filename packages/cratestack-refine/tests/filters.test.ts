import type { CrudFilters } from "@refinedev/core";
import { describe, expect, it } from "vitest";
import { toQueryFilters } from "../src/filters.js";
import { createCratestackDataProvider } from "../src/index.js";
import { createTestClient, WIDGET_SCHEMA } from "./support/client.js";

describe("toQueryFilters", () => {
  it("maps eq to a bare field query key", () => {
    expect(toQueryFilters([{ field: "name", operator: "eq", value: "gizmo" }])).toEqual({
      name: "gizmo",
    });
  });

  it("maps ne to field__ne", () => {
    expect(toQueryFilters([{ field: "name", operator: "ne", value: "gizmo" }])).toEqual({
      name__ne: "gizmo",
    });
  });

  it("maps in to field__in as a comma-joined list", () => {
    expect(toQueryFilters([{ field: "id", operator: "in", value: [1, 2, 3] }])).toEqual({
      id__in: "1,2,3",
    });
  });

  it("maps null/nnull to field__isNull true/false", () => {
    expect(toQueryFilters([{ field: "weight", operator: "null", value: true }])).toEqual({
      weight__isNull: "true",
    });
    expect(toQueryFilters([{ field: "weight", operator: "nnull", value: true }])).toEqual({
      weight__isNull: "false",
    });
  });

  it("throws on an operator with no cratestack equivalent instead of dropping the filter", () => {
    const unsupported: CrudFilters = [{ field: "name", operator: "endswith", value: "get" }];
    expect(() => toQueryFilters(unsupported)).toThrow(/no cratestack equivalent/);
  });

  it("throws on 'between', 'nin', and 'containss' too", () => {
    for (const operator of ["between", "nin", "containss"] as const) {
      expect(() => toQueryFilters([{ field: "name", operator, value: "x" }])).toThrow(
        /no cratestack equivalent/,
      );
    }
  });

  it("throws on a refine conditional filter group (or/and), not just unmapped field operators", () => {
    const group: CrudFilters = [
      { operator: "or", value: [{ field: "name", operator: "eq", value: "a" }] },
    ];
    expect(() => toQueryFilters(group)).toThrow(/filter groups have no cratestack equivalent/);
  });
});

describe("getList filter mapping against a real generated client", () => {
  it("threads a mapped filter through as a real query param and gets back the filtered rows", async () => {
    const { server, client } = createTestClient([WIDGET_SCHEMA]);
    server.seed("widgets", { id: 1, name: "gizmo", weight: 3 });
    server.seed("widgets", { id: 2, name: "gadget", weight: 5 });

    const provider = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    const result = await provider.getList({
      resource: "widgets",
      filters: [{ field: "weight", operator: "gte", value: 4 }],
    });

    const listRequest = server.requests.find(
      (r) => r.method === "GET" && r.url.includes("/widgets?"),
    );
    expect(listRequest?.url).toContain("weight__gte=4");
    expect(result.data).toEqual([{ id: 2, name: "gadget", weight: 5 }]);
  });

  it("rejects an unsupported operator before ever calling the real client", async () => {
    const { client } = createTestClient([WIDGET_SCHEMA]);
    const provider = createCratestackDataProvider({
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
