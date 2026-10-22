// RPC-transport counterpart to `decimal-list.test.ts` (cratestack#1155), on
// the pure-TypeScript JSON codec (`native_cbor: false`): a `Decimal[]` field
// on a model and on a `type` revives element by element. Run against the
// package generated from `tests/fixtures/decimal_list_rpc.cstack`.
import { describe, expect, it } from "vitest";
import { Decimal } from "./src/models.js";
import { LedgerApi, ProceduresApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";

function runtimeReturning(body: unknown): CratestackRpcRuntime {
  const fetchFn: typeof fetch = async () =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  return new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn });
}

describe("RPC Decimal[] on a model (cratestack#1155)", () => {
  it("revives each string element to a Decimal and passes a null element through", async () => {
    const ledger = await new LedgerApi(
      runtimeReturning({ id: "l1", entries: ["0.10", null, "1.00"] }),
    ).get("l1");

    expect(ledger.entries[0]).toBeInstanceOf(Decimal);
    expect((ledger.entries[0] as InstanceType<typeof Decimal>).equals(new Decimal("0.1"))).toBe(true);
    expect(ledger.entries[1]).toBeNull();
    expect(ledger.entries[2]).toBeInstanceOf(Decimal);
  });

  it("passes a null Decimal[] field through unchanged", async () => {
    const ledger = await new LedgerApi(runtimeReturning({ id: "l1", entries: null })).get("l1");

    expect(ledger.entries).toBeNull();
  });
});

describe("RPC Decimal[] on a type, reached through a procedure (cratestack#1155)", () => {
  it("revives each element of the type's Decimal[] field, and its scalar Decimal field", async () => {
    const result = await new ProceduresApi(
      runtimeReturning({ total: "1.10", parts: ["0.10", "1.00"] }),
    ).quote({});

    expect(result.total).toBeInstanceOf(Decimal);
    expect(result.parts[0]).toBeInstanceOf(Decimal);
    expect(result.parts[1]).toBeInstanceOf(Decimal);
    expect((result.parts[1] as InstanceType<typeof Decimal>).equals(new Decimal("1"))).toBe(true);
  });
});
