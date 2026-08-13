import { describe, expect, it } from "vitest";
import { createCratestackRpcDataProvider } from "../src/rpc-provider.js";
import { createRpcTestClient, LEDGER_RPC_SCHEMA } from "./support/rpc-client.js";

/** RPC sibling of `optimistic-locking.test.ts` — the decisive proof this
 *  whole change rests on: `@version` optimistic locking (If-Match)
 *  enforced identically over `transport rpc` as over REST, driven
 *  against a real generated RPC client. The claim under test is
 *  cratestack-macros' RPC dispatch arms
 *  (`crates/cratestack-macros/src/transport/rpc.rs`) passing the real
 *  HTTP `HeaderMap` straight through to the same
 *  `handle_update_ledgers_dispatch`/`handle_delete_ledgers_dispatch` fns
 *  REST uses, which read `If-Match` via `parse_if_match_version` — this
 *  file asserts on the *header the fake server actually received*, not
 *  just on the resulting status code, so a provider that "accidentally"
 *  passes by never sending `If-Match` at all cannot pass it. */
describe("If-Match optimistic locking over RPC (cratestack#493/#519/#538)", () => {
  function setup() {
    const { server, client } = createRpcTestClient([LEDGER_RPC_SCHEMA]);
    server.seed("Ledger", { id: 1, label: "checking", balance: 100, version: 1 });
    const provider = createCratestackRpcDataProvider({
      ledgers: { api: client.ledgers, primaryKey: "id", paged: true, versionField: "version" },
    });
    return { server, provider };
  }

  it("rejects a stale update with a distinguishable 412 conflict, and leaves the row untouched", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 });
    await provider.update({ resource: "ledgers", id: 1, variables: { balance: 150 } });

    await expect(
      provider.update({
        resource: "ledgers",
        id: 1,
        variables: { balance: 999 },
        meta: { ifMatch: 1 },
      }),
    ).rejects.toMatchObject({ statusCode: 412 });

    expect(server.row("Ledger", 1)).toMatchObject({ balance: 150, version: 2 });
  });

  it("sends a real If-Match header on the wire, and accepts a fresh update whose version matches", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 }); // version 1 cached

    const result = await provider.update({
      resource: "ledgers",
      id: 1,
      variables: { balance: 200 },
    });

    expect(result.data.balance).toBe(200);
    expect(server.row("Ledger", 1)).toMatchObject({ balance: 200, version: 2 });

    // The decisive assertion: the fake server actually observed the
    // header on an RPC call, not just "the mutation succeeded" (which a
    // provider that never sends If-Match at all would also achieve
    // against a version-less request the fake server happened to allow).
    const updateCall = server.requests.find((r) => r.opId === "model.Ledger.update");
    expect(updateCall).toBeDefined();
    expect(updateCall?.headers.get("If-Match")).toBe('"1"');
  });

  it("a stale If-Match surfaces as refine's expected 412 HttpError shape, not a generic failure", async () => {
    const { provider } = setup();
    await provider.getOne({ resource: "ledgers", id: 1 });
    await provider.update({ resource: "ledgers", id: 1, variables: { balance: 150 } });

    await expect(
      provider.update({
        resource: "ledgers",
        id: 1,
        variables: { balance: 999 },
        meta: { ifMatch: 1 },
      }),
    ).rejects.toMatchObject({
      statusCode: 412,
      message: "This record changed since it was loaded. Reload it and try again.",
    });
  });

  it("rejects a stale deleteOne with a 412, and leaves the row in place", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 });
    await provider.update({ resource: "ledgers", id: 1, variables: { balance: 150 } });

    await expect(
      provider.deleteOne({ resource: "ledgers", id: 1, meta: { ifMatch: 1 } }),
    ).rejects.toMatchObject({ statusCode: 412 });

    expect(server.row("Ledger", 1)).toBeDefined();
  });

  it("accepts a fresh deleteOne whose If-Match matches, and the fake server saw the header", async () => {
    const { server, provider } = setup();

    await provider.getOne({ resource: "ledgers", id: 1 });
    await provider.deleteOne({ resource: "ledgers", id: 1 });

    expect(server.row("Ledger", 1)).toBeUndefined();
    const deleteCall = server.requests.find((r) => r.opId === "model.Ledger.delete");
    expect(deleteCall?.headers.get("If-Match")).toBe('"1"');
  });

  it("throws rather than silently omitting If-Match when no version is known and no override is given", async () => {
    const { provider } = setup();
    await expect(
      provider.update({ resource: "ledgers", id: 1, variables: { balance: 1 } }),
    ).rejects.toThrow(/no known version/);
  });
});
