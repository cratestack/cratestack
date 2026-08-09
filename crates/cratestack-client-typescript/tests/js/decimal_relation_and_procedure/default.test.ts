// Real vitest proof for cratestack#499's remediation of #498's F2 (a
// procedure's own return type must revive `Decimal` fields — both a
// nested `type` field and a bare scalar `Decimal` return) and F5 (a
// relation-embedded `Decimal` field must revive too), against the real
// generated `default`-preset REST client — not a generated-text
// assertion. Copied alongside a generated package by
// `tests/decimal_relation_and_procedure_round_trip.rs`.
import { describe, expect, it } from "vitest";
import { Decimal } from "./src/models.js";
import { InvoiceApi, ProceduresApi } from "./src/client.js";
import { CratestackRuntime } from "./src/runtime.js";

function stubFetch(body: unknown): typeof fetch {
  return async () =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
}

describe("relation-embedded Decimal field revival (cratestack#499 F5)", () => {
  it("revives Invoice.customer.balance, not just Invoice's own direct fields", async () => {
    const runtime = new CratestackRuntime("http://example.invalid", {
      fetch: stubFetch({
        id: "inv_1",
        amountXaf: "1E-7",
        customerId: "cust_1",
        customer: { id: "cust_1", balance: "1.234567890123456789012345678901234567890E+10" },
      }),
    });
    const invoice = await new InvoiceApi(runtime).get("inv_1");

    expect(invoice.amountXaf).toBeInstanceOf(Decimal);
    expect((invoice.customer as unknown as { balance: unknown }).balance).toBeInstanceOf(Decimal);
    expect(
      ((invoice.customer as unknown as { balance: InstanceType<typeof Decimal> }).balance).toString(),
    ).toBe("12345678901.2345678901234567890123456789");
  });
});

describe("procedure return type Decimal revival (cratestack#499 F2)", () => {
  it("revives a Decimal field nested in a procedure's `type` return value", async () => {
    const runtime = new CratestackRuntime("http://example.invalid", {
      fetch: stubFetch({ price: "1E-7" }),
    });
    const result = await new ProceduresApi(runtime).quote({ reference: "q1" });

    expect(result.price).toBeInstanceOf(Decimal);
    expect((result.price as InstanceType<typeof Decimal>).equals(new Decimal("0.0000001"))).toBe(true);
  });

  it("revives a bare scalar Decimal return type (no wrapping object)", async () => {
    const wireValue = "1.234567890123456789012345678901234567890E+10";
    const runtime = new CratestackRuntime("http://example.invalid", {
      fetch: stubFetch(wireValue),
    });
    const result = await new ProceduresApi(runtime).quickQuote({ reference: "q2" });

    expect(result).toBeInstanceOf(Decimal);
    expect((result as InstanceType<typeof Decimal>).toString()).toBe(
      "12345678901.2345678901234567890123456789",
    );
  });
});
