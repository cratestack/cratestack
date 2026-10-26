// Run by `tests/ordinal_enums.rs` from the root of a package generated from
// `tests/fixtures/ordinal_enums.cstack` with `--swr`, so both layouts'
// revival is the code under test.
import * as plain from "./src/models";
import * as swr from "./src/swr/models/shared";

function eq(actual: unknown, expected: unknown, what: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${what}: expected ${e}, got ${a}`);
}

for (const [layout, m] of [["default", plain], ["swr", swr]] as const) {
  eq(
    m.reviveWireFields(
      { id: 1, status: 2, previous: null, channel: "card", refunds: [{ id: 9, paymentId: 1, status: 5 }] },
      "Payment",
    ),
    { id: 1, status: "Approved", previous: null, channel: "card", refunds: [{ id: 9, paymentId: 1, status: 5 }] },
    `${layout}: a Payment, whose Refund.status is a String`,
  );
  eq(
    m.reviveWireFields([{ id: 1, status: 1, previous: 5 }], "Payment"),
    [{ id: 1, status: "Pending", previous: "Refunded" }],
    `${layout}: a list`,
  );
  eq(
    m.revivePagedWireFields({ items: [{ id: 1, status: 5 }], totalCount: 1 }, "Payment"),
    { items: [{ id: 1, status: "Refunded" }], totalCount: 1 },
    `${layout}: a page`,
  );
  eq(
    m.reviveWireFields({ id: 9, paymentId: 1, status: "x", payment: { id: 1, status: 2 } }, "Refund"),
    { id: 9, paymentId: 1, status: "x", payment: { id: 1, status: "Approved" } },
    `${layout}: through a relation`,
  );
  eq(
    m.reviveWireFields({ current: 1, history: [1, 2, 5] }, "Summary"),
    { current: "Pending", history: ["Pending", "Approved", "Refunded"] },
    `${layout}: a type with an ordinal list`,
  );
  eq(m.reviveWireFields(5, "PaymentStatus"), "Refunded", `${layout}: a bare return`);
  // Tolerant decoding (§2.3): a peer still sending the name decodes.
  eq(m.reviveWireFields({ id: 1, status: "Approved" }, "Payment"), { id: 1, status: "Approved" }, `${layout}: a name`);
  // 3 is reserved and `Settled` was never a variant: both are refused.
  for (const bad of [3, "Settled"]) {
    let threw = false;
    try {
      m.reviveWireFields({ id: 1, status: bad }, "Payment");
    } catch (error) {
      threw = error instanceof TypeError && String(error).includes("Payment.status");
    }
    if (!threw) throw new Error(`${layout}: ${bad} must be refused`);
  }
}
console.log("ORDINAL_REVIVAL_OK");
