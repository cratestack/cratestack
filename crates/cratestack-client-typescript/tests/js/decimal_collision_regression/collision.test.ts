// Real vitest proof for cratestack#499's review remediation: the flat,
// name-keyed decimal revival scheme (an earlier version of `crate::decimal`)
// had a reachable field-name-collision hazard — `Order.total: Decimal` and
// related `Account.total: String`, `include`-ing the relation, either
// threw decoding a real (non-numeric) account reference or silently
// corrupted a numeric-looking one. The path-aware `wireShapes` registry
// fixes this: each type's own shape only ever describes that type's own
// fields, so `Account.total` is checked against *Account's* shape (which
// has no `total` key), never `Order`'s. Copied alongside a generated
// package by `tests/decimal_collision_regression.rs`, mirroring
// `tests/decimal_round_trip.rs`'s pattern.
import { describe, expect, it } from "vitest";
import { Decimal, reviveWireFields } from "./src/models.js";
import { OrderApi } from "./src/client.js";
import { CratestackRuntime } from "./src/runtime.js";

describe("wireShapes registry (cratestack#499 collision fix)", () => {
  it("does not convert a related model's same-named non-Decimal field — numeric-looking value", () => {
    const revived = reviveWireFields(
      { id: "ord_1", total: "42.50", account: { id: "acc_1", total: "00123" } },
      "Order",
    ) as { total: unknown; account: { total: unknown } };

    expect(revived.total).toBeInstanceOf(Decimal);
    expect((revived.total as InstanceType<typeof Decimal>).toString()).toBe("42.5");
    // The exact corruption this fix prevents: `Account.total` is a plain
    // `String` field that happens to share a name with `Order.total`
    // (a `Decimal`) — it must survive untouched, not become
    // `Decimal("123")` (losing its leading zeros).
    expect(revived.account.total).toBe("00123");
    expect(typeof revived.account.total).toBe("string");
  });

  it("does not throw on a related model's same-named non-Decimal, non-numeric field", () => {
    expect(() =>
      reviveWireFields(
        { id: "ord_2", total: "42.50", account: { id: "acc_2", total: "ACC-00123" } },
        "Order",
      ),
    ).not.toThrow();

    const revived = reviveWireFields(
      { id: "ord_2", total: "42.50", account: { id: "acc_2", total: "ACC-00123" } },
      "Order",
    ) as { account: { total: unknown } };
    expect(revived.account.total).toBe("ACC-00123");
  });

  it("still revives Account's own Decimal field correctly when Account is the root", () => {
    // Sanity check the fixture the other way around: Account has no
    // Decimal field in this schema, so decoding it directly must leave
    // `total` untouched (it's a String there).
    const revived = reviveWireFields({ id: "acc_1", total: "00123" }, "Account") as {
      total: unknown;
    };
    expect(revived.total).toBe("00123");
  });

  it("the real generated OrderApi.get() revives Order.total but not Account.total", async () => {
    const stubFetch: typeof fetch = async () =>
      new Response(
        JSON.stringify({
          id: "ord_3",
          total: "1E-7",
          accountId: "acc_3",
          account: { id: "acc_3", total: "00099" },
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );

    const runtime = new CratestackRuntime("http://example.invalid", { fetch: stubFetch });
    const order = await new OrderApi(runtime).get("ord_3");

    expect(order.total).toBeInstanceOf(Decimal);
    expect((order.total as InstanceType<typeof Decimal>).equals(new Decimal("0.0000001"))).toBe(true);
    expect((order.account as unknown as { total: unknown }).total).toBe("00099");
  });
});
