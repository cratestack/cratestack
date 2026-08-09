// Real vitest proof for cratestack#498's correctness requirements 1-3
// (decode accepts both plain and scientific notation into equal values;
// encode round-trips; precision survives beyond `rust_decimal`'s
// capacity) — run for real against the generated `decimal_scalar.cstack`
// package (`crates/cratestack-client-typescript/tests/fixtures/
// decimal_scalar.cstack`), not asserted as generated-text-contains-X in
// Rust. Copied alongside a generated package by
// `tests/decimal_round_trip.rs`, mirroring `swr_hooks_invalidation.rs`'s
// "generate a real package, `npm install`, run real vitest" pattern.
import { describe, expect, it } from "vitest";
import { Decimal, reviveDecimalFields } from "./src/models.js";
import { InvoiceApi } from "./src/client.js";
import { CratestackRuntime } from "./src/runtime.js";

describe("Decimal parsing (cratestack#498 requirement 1)", () => {
  it("parses plain and scientific notation for the same value into equal Decimals", () => {
    const plain = new Decimal("0.0000001");
    const scientific = new Decimal("1E-7");
    expect(plain.equals(scientific)).toBe(true);
  });

  it("preserves precision beyond rust_decimal's ~28-29 significant-digit capacity (requirement 3)", () => {
    // Mirrors `crates/cratestack-pg/tests/decimal_bigdecimal_backend.rs`'s
    // `decimal_round_trips_beyond_rust_decimal_capacity_under_bigdecimal_backend`
    // — a 40-significant-digit value, scientific notation on the wire
    // (what `bigdecimal`'s `Display` would actually emit for it).
    const wireValue = "1.234567890123456789012345678901234567890E+10";
    const decoded = new Decimal(wireValue);
    const reEncoded = decoded.toString();
    // Requirement 2: string form may normalize (plain positional, not
    // scientific — see `models.ts.j2`'s `Decimal` export doc comment for
    // why plain notation is forced), but the *value* must round-trip.
    expect(reEncoded).toBe("12345678901.2345678901234567890123456789");
    expect(new Decimal(reEncoded).equals(decoded)).toBe(true);
  });
});

describe("reviveDecimalFields (the generated client's decode-side hook)", () => {
  it("is a no-op for a shape name with no decimalShapes registry entry", () => {
    const value = { amountXaf: "1E-7" };
    expect(reviveDecimalFields(value, "NotARegisteredShape")).toBe(value);
  });

  it("turns matching string fields into real Decimal instances, leaves others untouched", () => {
    const revived = reviveDecimalFields(
      { id: "inv_1", reference: "INV-1", amountXaf: "1E-7", discountXaf: null },
      "Invoice",
    ) as { id: string; amountXaf: unknown; discountXaf: unknown };

    expect(revived.id).toBe("inv_1");
    expect(revived.amountXaf).toBeInstanceOf(Decimal);
    expect((revived.amountXaf as InstanceType<typeof Decimal>).equals(new Decimal("0.0000001"))).toBe(true);
    expect(revived.discountXaf).toBeNull();
  });

  it("revives every item of an array response (the `list()` shape)", () => {
    const revived = reviveDecimalFields(
      [{ amountXaf: "1E-7" }, { amountXaf: "0.0000001" }],
      "Invoice",
    ) as Array<{ amountXaf: InstanceType<typeof Decimal> }>;

    expect(revived[0]!.amountXaf.equals(revived[1]!.amountXaf)).toBe(true);
  });
});

describe("InvoiceApi.get (the real generated REST client, requirement 6 — REST)", () => {
  it("decodes a server response with scientific notation into a real Decimal (requirement 1, end to end)", async () => {
    const stubFetch: typeof fetch = async () =>
      new Response(
        JSON.stringify({
          id: "inv_1",
          reference: "INV-1",
          amountXaf: "1E-7",
          discountXaf: null,
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );

    const runtime = new CratestackRuntime("http://example.invalid", { fetch: stubFetch });
    const api = new InvoiceApi(runtime);
    const invoice = await api.get("inv_1");

    expect(invoice.amountXaf).toBeInstanceOf(Decimal);
    expect((invoice.amountXaf as InstanceType<typeof Decimal>).equals(new Decimal("0.0000001"))).toBe(true);
    expect(invoice.discountXaf).toBeNull();

    // Requirement 2: re-encoding what the client decoded is accepted
    // back unchanged in value (string form normalizes to plain
    // notation, matching what a `decimal-rust-decimal` server emits and
    // parses natively).
    const reEncoded = JSON.parse(JSON.stringify({ amountXaf: invoice.amountXaf }));
    expect(reEncoded.amountXaf).toBe("0.0000001");
  });

  it("decodes a value beyond rust_decimal's capacity from a decimal-bigdecimal-shaped response (requirement 3)", async () => {
    const wireValue = "1.234567890123456789012345678901234567890E+10";
    const stubFetch: typeof fetch = async () =>
      new Response(
        JSON.stringify({
          id: "inv_2",
          reference: "INV-2",
          amountXaf: wireValue,
          discountXaf: null,
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );

    const runtime = new CratestackRuntime("http://example.invalid", { fetch: stubFetch });
    const api = new InvoiceApi(runtime);
    const invoice = await api.get("inv_2");

    expect((invoice.amountXaf as InstanceType<typeof Decimal>).toString()).toBe(
      "12345678901.2345678901234567890123456789",
    );
  });
});
