// `swr`-preset counterpart to `default.test.ts` — proves cratestack#499's
// remediation of #498's F3 (the `swr` preset's per-model plain functions
// now call `reviveDecimalFields` too, closing the documented "type-correct
// but not yet revived" gap), F2 (procedure return type revival), and F5
// (relation-embedded field revival), against the real generated
// per-model-file layout, not a generated-text assertion.
import { describe, expect, it } from "vitest";
import { Decimal } from "./src/swr/models/shared.js";
import { getInvoice } from "./src/swr/models/invoice.js";
import { quote, quickQuote } from "./src/swr/procedures.js";
import { CratestackRuntime } from "./src/swr/runtime.js";

function stubFetch(body: unknown): typeof fetch {
  return async () =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
}

describe("swr preset decode-side revival (cratestack#499 F3/F5)", () => {
  it("getInvoice revives both the model's own field and its relation-embedded field", async () => {
    const runtime = new CratestackRuntime("http://example.invalid", {
      fetch: stubFetch({
        id: "inv_1",
        amountXaf: "1E-7",
        customerId: "cust_1",
        customer: { id: "cust_1", balance: "1.234567890123456789012345678901234567890E+10" },
      }),
    });
    const invoice = await getInvoice(runtime, "inv_1");

    expect(invoice.amountXaf).toBeInstanceOf(Decimal);
    expect((invoice.customer as unknown as { balance: unknown }).balance).toBeInstanceOf(Decimal);
    expect(
      ((invoice.customer as unknown as { balance: InstanceType<typeof Decimal> }).balance).toString(),
    ).toBe("12345678901.2345678901234567890123456789");
  });
});

describe("swr preset procedure return type revival (cratestack#499 F2)", () => {
  it("revives a Decimal field nested in a procedure's `type` return value", async () => {
    const runtime = new CratestackRuntime("http://example.invalid", {
      fetch: stubFetch({ price: "1E-7" }),
    });
    const result = await quote(runtime, { reference: "q1" });

    expect(result.price).toBeInstanceOf(Decimal);
    expect((result.price as InstanceType<typeof Decimal>).equals(new Decimal("0.0000001"))).toBe(true);
  });

  it("revives a bare scalar Decimal return type", async () => {
    const wireValue = "1.234567890123456789012345678901234567890E+10";
    const runtime = new CratestackRuntime("http://example.invalid", {
      fetch: stubFetch(wireValue),
    });
    const result = await quickQuote(runtime, { reference: "q2" });

    expect(result).toBeInstanceOf(Decimal);
    expect((result as InstanceType<typeof Decimal>).toString()).toBe(
      "12345678901.2345678901234567890123456789",
    );
  });
});
