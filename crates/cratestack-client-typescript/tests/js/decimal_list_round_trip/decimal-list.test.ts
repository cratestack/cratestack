// Real vitest proof for cratestack#1155 on the REST transport, default
// layout: a `Decimal[]` field, on a model and on a `type`, is revived
// element by element to real `Decimal`s, while a `null` element, a `null`
// array and a scalar `Decimal` field are unchanged. Run for real against the
// package generated from `tests/fixtures/decimal_list.cstack`, copied
// alongside it by `tests/decimal_list_round_trip.rs`.
import { describe, expect, it } from "vitest";
import { Decimal } from "./src/models.js";
import { LedgerApi, ProceduresApi } from "./src/client.js";
import { CratestackRuntime } from "./src/runtime.js";

function runtimeReturning(body: unknown): CratestackRuntime {
  const fetchFn: typeof fetch = async () =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  return new CratestackRuntime("http://example.invalid", { fetch: fetchFn });
}

describe("Decimal[] on a model (cratestack#1155)", () => {
  it("revives each string element to a Decimal and passes a null element through", async () => {
    const ledger = await new LedgerApi(
      runtimeReturning({ id: "l1", entries: ["0.10", null, "1.00"] }),
    ).get("l1");

    expect(ledger.entries[0]).toBeInstanceOf(Decimal);
    expect((ledger.entries[0] as InstanceType<typeof Decimal>).equals(new Decimal("0.1"))).toBe(true);
    expect(ledger.entries[1]).toBeNull();
    expect(ledger.entries[2]).toBeInstanceOf(Decimal);
    expect((ledger.entries[2] as InstanceType<typeof Decimal>).plus("1").toString()).toBe("2");
  });

  it("passes a null Decimal[] field through unchanged", async () => {
    const ledger = await new LedgerApi(runtimeReturning({ id: "l1", entries: null })).get("l1");

    expect(ledger.entries).toBeNull();
  });
});

describe("Decimal[] on a type, reached through a procedure (cratestack#1155)", () => {
  it("revives each element of the type's Decimal[] field, and its scalar Decimal field", async () => {
    const quote = await new ProceduresApi(
      runtimeReturning({ total: "1.10", parts: ["0.10", "1.00"] }),
    ).quote({});

    expect(quote.total).toBeInstanceOf(Decimal);
    expect(quote.parts[0]).toBeInstanceOf(Decimal);
    expect(quote.parts[1]).toBeInstanceOf(Decimal);
    expect((quote.parts[0] as InstanceType<typeof Decimal>).equals(new Decimal("0.10"))).toBe(true);
    expect((quote.parts[0] as InstanceType<typeof Decimal>).plus(quote.parts[1] as InstanceType<typeof Decimal>).toString()).toBe("1.1");
  });
});
