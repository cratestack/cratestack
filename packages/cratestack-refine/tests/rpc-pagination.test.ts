import { describe, expect, it } from "vitest";
import { createCratestackRpcDataProvider } from "../src/rpc-provider.js";
import { createRpcTestClient, LEDGER_RPC_SCHEMA, WIDGET_RPC_SCHEMA } from "./support/rpc-client.js";

/** RPC sibling of `pagination.test.ts`. */
describe("getList pagination against a real Page<T> response over RPC", () => {
  it("computes limit/offset from { current, pageSize } and returns the server's real totalCount", async () => {
    const { server, client } = createRpcTestClient([LEDGER_RPC_SCHEMA]);
    for (let i = 1; i <= 7; i++) {
      server.seed("Ledger", { id: i, label: `ledger-${i}`, balance: i * 10, version: 1 });
    }

    const provider = createCratestackRpcDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });

    const result = await provider.getList({
      resource: "ledgers",
      pagination: { currentPage: 2, pageSize: 3, mode: "server" },
      sorters: [{ field: "id", order: "asc" }],
    });

    const listCall = server.requests.find((r) => r.opId === "model.Ledger.list");
    expect(listCall?.body).toMatchObject({ limit: 3, offset: 3, sort: "id" });

    expect(result.total).toBe(7);
    expect(result.data.map((row) => row.id)).toEqual([4, 5, 6]);
  });

  it("computes offset 0 for page 1", async () => {
    const { server, client } = createRpcTestClient([LEDGER_RPC_SCHEMA]);
    server.seed("Ledger", { id: 1, label: "only", balance: 1, version: 1 });

    const provider = createCratestackRpcDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });

    await provider.getList({ resource: "ledgers", pagination: { currentPage: 1, pageSize: 10 } });

    const listCall = server.requests.find((r) => r.opId === "model.Ledger.list");
    expect(listCall?.body).toMatchObject({ offset: 0 });
  });

  it("does not send limit/offset for a non-@@paged resource, and total degrades to the response's own length", async () => {
    const { server, client } = createRpcTestClient([WIDGET_RPC_SCHEMA]);
    server.seed("Widget", { id: 1, name: "gizmo", weight: null });
    server.seed("Widget", { id: 2, name: "gadget", weight: null });

    const provider = createCratestackRpcDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    const result = await provider.getList({
      resource: "widgets",
      pagination: { currentPage: 2, pageSize: 1 },
    });

    const listCall = server.requests.find((r) => r.opId === "model.Widget.list");
    expect(listCall?.body).not.toHaveProperty("limit");
    expect(listCall?.body).not.toHaveProperty("offset");
    expect(result.total).toBe(2);
    expect(result.data).toHaveLength(2);
  });

  it("honors pagination: { mode: 'off' } even on a @@paged resource", async () => {
    const { server, client } = createRpcTestClient([LEDGER_RPC_SCHEMA]);
    for (let i = 1; i <= 5; i++) {
      server.seed("Ledger", { id: i, label: `ledger-${i}`, balance: i, version: 1 });
    }

    const provider = createCratestackRpcDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });

    const result = await provider.getList({
      resource: "ledgers",
      pagination: { currentPage: 1, pageSize: 2, mode: "off" },
    });

    const listCall = server.requests.find((r) => r.opId === "model.Ledger.list");
    expect(listCall?.body).not.toHaveProperty("limit");
    expect(result.total).toBe(5);
    expect(result.data).toHaveLength(5);
  });
});
