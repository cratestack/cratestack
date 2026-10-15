// `--swr`-layout counterpart to `decimal-list.test.ts` (cratestack#1155): the
// REST transport's file-per-model functions and procedure functions revive a
// `Decimal[]` field on a model and on a `type` element by element. Run
// against the same `decimal_list.cstack` package, `src/swr/` layout.
import { describe, expect, it } from "vitest";
import { Decimal } from "./src/swr/models/shared.js";
import { getLedger } from "./src/swr/models/ledger.js";
import { quote } from "./src/swr/procedures.js";
import { CratestackRuntime } from "./src/swr/runtime.js";

function runtimeReturning(body: unknown): CratestackRuntime {
  const fetchFn: typeof fetch = async () =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  return new CratestackRuntime("http://example.invalid", { fetch: fetchFn });
}

describe("swr layout: Decimal[] on a model (cratestack#1155)", () => {
  it("revives each string element to a Decimal and passes a null element through", async () => {
    const ledger = await getLedger(runtimeReturning({ id: "l1", entries: ["0.10", null, "1.00"] }), "l1");

    expect(ledger.entries[0]).toBeInstanceOf(Decimal);
    expect((ledger.entries[0] as InstanceType<typeof Decimal>).equals(new Decimal("0.1"))).toBe(true);
    expect(ledger.entries[1]).toBeNull();
    expect(ledger.entries[2]).toBeInstanceOf(Decimal);
  });

  it("passes a null Decimal[] field through unchanged", async () => {
    const ledger = await getLedger(runtimeReturning({ id: "l1", entries: null }), "l1");

    expect(ledger.entries).toBeNull();
  });
});

describe("swr layout: Decimal[] on a type, reached through a procedure (cratestack#1155)", () => {
  it("revives each element of the type's Decimal[] field, and its scalar Decimal field", async () => {
    const result = await quote(runtimeReturning({ total: "1.10", parts: ["0.10", "1.00"] }), {});

    expect(result.total).toBeInstanceOf(Decimal);
    expect(result.parts[0]).toBeInstanceOf(Decimal);
    expect(result.parts[1]).toBeInstanceOf(Decimal);
    expect((result.parts[1] as InstanceType<typeof Decimal>).equals(new Decimal("1"))).toBe(true);
  });
});
