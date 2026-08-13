import { describe, expect, it } from "vitest";
import { createCratestackDataProvider } from "../src/index.js";
import { createTestClient, LEDGER_SCHEMA } from "./support/client.js";

/** The decisive test cratestack#571 calls out by name: a STALE update
 *  must be rejected — proving a fresh one succeeds is not enough, since
 *  a dataProvider that never sends `If-Match` at all would also pass
 *  that half. This drives the real generated `LedgerApi.update`/
 *  `.delete` against a fake server that actually enforces the
 *  `If-Match` contract (cratestack#493/#519/#538), not a mock that
 *  always says yes. */
describe("If-Match optimistic locking (cratestack#493/#519/#538)", () => {
  function setup() {
    const { server, client } = createTestClient([LEDGER_SCHEMA]);
    server.seed("ledgers", { id: 1, label: "checking", balance: 100, version: 1 });
    const provider = createCratestackDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });
    return { server, provider };
  }

  it("rejects a stale update with a distinguishable 412 conflict, and leaves the row untouched", async () => {
    const { server, provider } = setup();

    // Populate the version cache the way refine really would: fetch the
    // record before editing it.
    await provider.getOne({ resource: "ledgers", id: 1 });

    // Someone else updates the row first, bumping version 1 -> 2 server-side.
    await provider.update({ resource: "ledgers", id: 1, variables: { balance: 150 } });

    // A second editor who loaded the record back when it was still
    // version 1 tries to save — `meta.ifMatch: 1` simulates exactly what
    // their own dataProvider instance's version cache would still hold
    // (this provider's own cache already advanced to 2 above; the point
    // of this test is what the SERVER does with a stale value, which
    // `meta.ifMatch` lets us force deterministically).
    await expect(
      provider.update({
        resource: "ledgers",
        id: 1,
        variables: { balance: 999 },
        meta: { ifMatch: 1 },
      }),
    ).rejects.toMatchObject({ statusCode: 412 });

    // The row was NOT mutated by the rejected request.
    expect(server.row("ledgers", 1)).toMatchObject({ balance: 150, version: 2 });
  });

  it("accepts a fresh update whose If-Match matches the row's current version", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 }); // version 1 cached

    const result = await provider.update({
      resource: "ledgers",
      id: 1,
      variables: { balance: 200 },
    });

    expect(result.data.balance).toBe(200);
    expect(server.row("ledgers", 1)).toMatchObject({ balance: 200, version: 2 });
    const updateRequest = server.requests.find((r) => r.method === "PATCH");
    expect(updateRequest?.headers.get("If-Match")).toBe('"1"');
  });

  it("rejects a stale deleteOne with a 412, and leaves the row in place", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 }); // version 1 cached
    await provider.update({ resource: "ledgers", id: 1, variables: { balance: 150 } }); // bumps to version 2 server-side

    await expect(
      provider.deleteOne({ resource: "ledgers", id: 1, meta: { ifMatch: 1 } }),
    ).rejects.toMatchObject({ statusCode: 412 });

    expect(server.row("ledgers", 1)).toBeDefined();
  });

  it("accepts a fresh deleteOne whose If-Match matches", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 });
    await provider.deleteOne({ resource: "ledgers", id: 1 });

    expect(server.row("ledgers", 1)).toBeUndefined();
    const deleteRequest = server.requests.find((r) => r.method === "DELETE");
    expect(deleteRequest?.headers.get("If-Match")).toBe('"1"');
  });

  it("throws rather than silently omitting If-Match when no version is known and no override is given", async () => {
    const { provider } = setup();
    // No prior getOne/getList — the version cache has nothing for id 1.
    await expect(
      provider.update({ resource: "ledgers", id: 1, variables: { balance: 1 } }),
    ).rejects.toThrow(/no known version/);
  });
});
