// `swr`-layout counterpart to `bigint.test.ts` (ADR 0019). The SWR layout
// carries its OWN copy of the revival machinery in `src/swr/models/shared.ts`
// (it has no `models.ts` of its own), so the default layout's tests prove
// nothing about it: this file drives the per-model plain functions and the
// procedure functions of the generated `src/swr/` tree.
import { describe, expect, it } from "vitest";
import { reviveWireFields, reviveWireScalar } from "./src/swr/models/shared.js";
import { createLedger, getLedger, listLedgers } from "./src/swr/models/ledger.js";
import { getCounter } from "./src/swr/models/counter.js";
import { balance, history, scale, totals } from "./src/swr/procedures.js";
import { CratestackRuntime } from "./src/swr/runtime.js";

const BOUNDARY = ["9223372036854775807", "-9223372036854775808", "9007199254740993"] as const;

function runtimeReturning(responseBody: unknown): { runtime: CratestackRuntime; bodies: string[] } {
  const bodies: string[] = [];
  const fetchFn: typeof fetch = async (_input, init) => {
    if (typeof init?.body === "string") {
      bodies.push(init.body);
    }
    return new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
  return { runtime: new CratestackRuntime("http://example.invalid", { fetch: fetchFn }), bodies };
}

describe("swr layout: decode", () => {
  it.each(BOUNDARY)("getLedger revives %s to an exact bigint", async (wire) => {
    const ledger = await getLedger(runtimeReturning({ amountE8: wire }).runtime, "l1");
    expect(ledger.amountE8).toBe(BigInt(wire));
  });

  it("revives an optional, a list, and a Page<Ledger> envelope's items", async () => {
    const ledger = await getLedger(
      runtimeReturning({ feeE8: null, tiers: [...BOUNDARY] }).runtime,
      "l1",
    );
    expect(ledger.feeE8).toBeNull();
    expect(ledger.tiers).toEqual(BOUNDARY.map((wire) => BigInt(wire)));

    const page = await listLedgers(
      runtimeReturning({ items: [{ amountE8: "-1" }], totalCount: 1, pageInfo: {} }).runtime,
    );
    expect(page.items[0]?.amountE8).toBe(-1n);
  });

  it("revives a relation-embedded BigInt and a BigInt @id", async () => {
    const ledger = await getLedger(
      runtimeReturning({ entries: [{ deltaE8: "9007199254740993", amountE8: "007" }] }).runtime,
      "l1",
    );
    expect(ledger.entries?.[0]?.deltaE8).toBe(9007199254740993n);
    expect(ledger.entries?.[0]?.amountE8).toBe("007");

    const counter = await getCounter(runtimeReturning({ id: "5", hitsE8: "6" }).runtime, 5n);
    expect(counter).toEqual({ id: 5n, hitsE8: 6n });
  });

  it("procedure returns: bare bigint, bigint[], and a type holding a BigInt", async () => {
    expect(await balance(runtimeReturning("9223372036854775807").runtime, { reference: "r" })).toBe(
      9223372036854775807n,
    );
    expect(await history(runtimeReturning([...BOUNDARY]).runtime, { reference: "r" })).toEqual(
      BOUNDARY.map((wire) => BigInt(wire)),
    );
    expect(
      await totals(runtimeReturning({ grossE8: "-1", history: ["2"] }).runtime, { reference: "r" }),
    ).toEqual({ grossE8: -1n, history: [2n] });
  });
});

describe("swr layout: a number at a BigInt key throws", () => {
  it("in a model, a nested model, and a bare procedure return", async () => {
    await expect(getLedger(runtimeReturning({ amountE8: 5 }).runtime, "l1")).rejects.toThrow(
      /BigInt field Ledger\.amountE8: expected a canonical decimal string, got number 5/,
    );
    await expect(
      getLedger(runtimeReturning({ entries: [{ deltaE8: 5 }] }).runtime, "l1"),
    ).rejects.toThrow(/Entry\.deltaE8/);
    await expect(balance(runtimeReturning(5).runtime, { reference: "r" })).rejects.toThrow(
      /procedure return/,
    );
  });

  it("refuses non-canonical strings and a typed bigint, as the default layout does", () => {
    for (const wire of ["+5", "007", "-0", " 1", "9223372036854775808"]) {
      expect(() => reviveWireFields({ amountE8: wire }, "Ledger")).toThrow(TypeError);
    }
    expect(() => reviveWireFields({ amountE8: 5n }, "Ledger")).toThrow(/got bigint 5/);
    expect(() => reviveWireScalar(5, "bigint")).toThrow(TypeError);
  });
});

describe("swr layout: encode", () => {
  it("createLedger and a bigint procedure argument send the canonical string", async () => {
    const created = runtimeReturning({ id: "l1" });
    await createLedger(created.runtime, {
      reference: "r",
      amountE8: 9223372036854775807n,
      feeE8: null,
      tiers: [1n],
    });
    expect(created.bodies[0]).toBe(
      '{"reference":"r","amountE8":"9223372036854775807","feeE8":null,"tiers":["1"]}',
    );

    const scaled = runtimeReturning("1");
    await scale(scaled.runtime, { amountE8: -9223372036854775808n, factorE8: 2n });
    expect(scaled.bodies[0]).toBe('{"amountE8":"-9223372036854775808","factorE8":"2"}');
  });
});
