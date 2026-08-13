import { describe, expect, it } from "vitest";
import { createCratestackDataProvider } from "../src/index.js";
import { createTestClient, LEDGER_SCHEMA, WIDGET_SCHEMA } from "./support/client.js";

describe("getList pagination against a real Page<T> response", () => {
  it("computes limit/offset from { current, pageSize } and returns the server's real totalCount", async () => {
    const { server, client } = createTestClient([LEDGER_SCHEMA]);
    for (let i = 1; i <= 7; i++) {
      server.seed("ledgers", { id: i, label: `ledger-${i}`, balance: i * 10, version: 1 });
    }

    const provider = createCratestackDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });

    // Page 2 of pageSize 3 over 7 rows: rows 4,5,6 (offset 3, limit 3).
    const result = await provider.getList({
      resource: "ledgers",
      pagination: { currentPage: 2, pageSize: 3, mode: "server" },
      sorters: [{ field: "id", order: "asc" }],
    });

    const listRequest = server.requests.find(
      (r) => r.method === "GET" && r.url.includes("/ledgers?"),
    );
    expect(listRequest?.url).toContain("limit=3");
    expect(listRequest?.url).toContain("offset=3");

    expect(result.total).toBe(7); // the real Page<Ledger>.totalCount, not items.length
    expect(result.data.map((row) => row.id)).toEqual([4, 5, 6]);
  });

  it("computes offset 0 for page 1", async () => {
    const { server, client } = createTestClient([LEDGER_SCHEMA]);
    server.seed("ledgers", { id: 1, label: "only", balance: 1, version: 1 });

    const provider = createCratestackDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });

    await provider.getList({ resource: "ledgers", pagination: { currentPage: 1, pageSize: 10 } });

    const listRequest = server.requests.find(
      (r) => r.method === "GET" && r.url.includes("/ledgers?"),
    );
    expect(listRequest?.url).toContain("offset=0");
  });

  it("does not send limit/offset for a non-@@paged resource, and total degrades to the response's own length", async () => {
    const { server, client } = createTestClient([WIDGET_SCHEMA]);
    server.seed("widgets", { id: 1, name: "gizmo", weight: null });
    server.seed("widgets", { id: 2, name: "gadget", weight: null });

    const provider = createCratestackDataProvider({
      widgets: { api: client.widgets, primaryKey: "id", paged: false },
    });

    const result = await provider.getList({
      resource: "widgets",
      pagination: { currentPage: 2, pageSize: 1 },
    });

    const listRequest = server.requests.find(
      (r) => r.method === "GET" && r.url.includes("/widgets"),
    );
    expect(listRequest?.url).not.toContain("limit=");
    expect(listRequest?.url).not.toContain("offset=");
    // Every row came back (no server-side paging happened); total honestly
    // reports that, rather than claiming a page-2-of-something total.
    expect(result.total).toBe(2);
    expect(result.data).toHaveLength(2);
  });

  it("honors pagination: { mode: 'off' } even on a @@paged resource", async () => {
    const { server, client } = createTestClient([LEDGER_SCHEMA]);
    for (let i = 1; i <= 5; i++) {
      server.seed("ledgers", { id: i, label: `ledger-${i}`, balance: i, version: 1 });
    }

    const provider = createCratestackDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });

    const result = await provider.getList({
      resource: "ledgers",
      pagination: { currentPage: 1, pageSize: 2, mode: "off" },
    });

    const listRequest = server.requests.find(
      (r) => r.method === "GET" && r.url.includes("/ledgers"),
    );
    expect(listRequest?.url).not.toContain("limit=");
    // Still a real Page response, so total is still the server's real count.
    expect(result.total).toBe(5);
    expect(result.data).toHaveLength(5);
  });
});
