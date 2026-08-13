import { describe, expect, it } from "vitest";
import { createCratestackDataProvider } from "../src/index.js";
import { createTestClient, WIDGET_SCHEMA } from "./support/client.js";

describe("createCratestackDataProvider", () => {
  it("throws a clear error for a resource with no matching config, instead of a confusing undefined crash", async () => {
    const { client } = createTestClient([WIDGET_SCHEMA]);
    const provider = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    await expect(provider.getOne({ resource: "unknown", id: 1 })).rejects.toThrow(
      /no cratestack resource configured/,
    );
  });

  it("getMany fetches all requested ids in a single list() call, not N getOne calls", async () => {
    const { server, client } = createTestClient([WIDGET_SCHEMA]);
    server.seed("widgets", { id: 1, name: "a", weight: null });
    server.seed("widgets", { id: 2, name: "b", weight: null });
    server.seed("widgets", { id: 3, name: "c", weight: null });
    const provider = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    const result = await provider.getMany!({ resource: "widgets", ids: [1, 3] });

    expect(result.data.map((row) => row.id).sort()).toEqual([1, 3]);
    const getRequests = server.requests.filter((r) => r.method === "GET");
    expect(getRequests).toHaveLength(1);
  });

  it("create throws when the model has no generated create route", async () => {
    const { client } = createTestClient([WIDGET_SCHEMA]);
    // Structurally identical to a real generated class whose model has no
    // @@allow("create", ...) — `create` is simply absent, matching the
    // real generator's `model_allows_create` gate.
    const noCreateApi = { ...client.widgets, create: undefined };
    const provider = createCratestackDataProvider({
      widgets: { api: noCreateApi, primaryKey: "id", paged: false },
    });

    await expect(
      provider.create({ resource: "widgets", variables: { id: 1, name: "x" } }),
    ).rejects.toThrow(/no generated create route/);
  });

  it("createMany/updateMany/deleteMany are implemented as N real round trips over the single-record methods", async () => {
    const { server, client } = createTestClient([WIDGET_SCHEMA]);
    const provider = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    const created = await provider.createMany!({
      resource: "widgets",
      variables: [
        { id: 1, name: "a" },
        { id: 2, name: "b" },
      ],
    });
    expect(created.data).toHaveLength(2);
    expect(server.requests.filter((r) => r.method === "POST")).toHaveLength(2);

    const updated = await provider.updateMany!({
      resource: "widgets",
      ids: [1, 2],
      variables: { name: "renamed" },
    });
    expect(updated.data.every((row) => row.name === "renamed")).toBe(true);
    expect(server.requests.filter((r) => r.method === "PATCH")).toHaveLength(2);

    const deleted = await provider.deleteMany!({ resource: "widgets", ids: [1, 2] });
    expect(deleted.data.map((row) => row.id).sort()).toEqual([1, 2]);
    expect(server.row("widgets", 1)).toBeUndefined();
    expect(server.row("widgets", 2)).toBeUndefined();
  });

  it("custom() dispatches to a configured procedure by name", async () => {
    const { client } = createTestClient([WIDGET_SCHEMA]);
    const provider = createCratestackDataProvider(
      { widgets: { api: client.widgets, primaryKey: "id", paged: false } },
      { procedures: { echoName: async (args) => ({ echoed: (args as { name: string }).name }) } },
    );

    const result = await provider.custom!({
      url: "",
      method: "post",
      meta: { procedure: "echoName" },
      payload: { name: "hi" },
    });

    expect(result.data).toEqual({ echoed: "hi" });
  });

  it("custom() throws when meta.procedure names nothing configured", async () => {
    const { client } = createTestClient([WIDGET_SCHEMA]);
    const provider = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    await expect(
      provider.custom!({ url: "", method: "post", meta: { procedure: "nope" } }),
    ).rejects.toThrow(/needs meta: \{ procedure/);
  });

  it("getApiUrl defaults to an empty string, or a caller-supplied callback", () => {
    const { client } = createTestClient([WIDGET_SCHEMA]);
    const withDefault = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });
    expect(withDefault.getApiUrl()).toBe("");

    const withCallback = createCratestackDataProvider(
      { widgets: { api: client.widgets, primaryKey: "id", paged: false } },
      { getApiUrl: () => "https://example.test/api" },
    );
    expect(withCallback.getApiUrl()).toBe("https://example.test/api");
  });
});
