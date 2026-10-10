// Real vitest proof for ADR 0019 on the REST transport: the generated
// client holds a `BigInt` field as an exact `bigint`, decodes the canonical
// decimal string the server writes, refuses a number at that key, and sends
// a `bigint` back as the same string. Run for real against the package
// generated from `tests/fixtures/bigint_scalar.cstack`, copied alongside it
// by `tests/bigint_round_trip.rs`. Modelled on `tests/js/decimal_round_trip`.
import { describe, expect, it } from "vitest";
import { Decimal, encodeBinaryAsJson, encodeWireFields, reviveWireFields, reviveWireScalar } from "./src/models.js";
import { CounterApi, EntryApi, LedgerApi, ProceduresApi } from "./src/client.js";
import { CratestackRuntime } from "./src/runtime.js";

// The three boundary values the cross-language fixtures pin (ADR 0019 D2):
// i64::MAX, i64::MIN, and 2^53 + 1, the first integer a double cannot hold.
const BOUNDARY = [
  "9223372036854775807",
  "-9223372036854775808",
  "9007199254740993",
] as const;

interface Sent {
  url: string;
  method: string;
  body: string | null;
}

function runtimeReturning(responseBody: unknown): { runtime: CratestackRuntime; sent: Sent[] } {
  const sent: Sent[] = [];
  const fetchFn: typeof fetch = async (input, init) => {
    sent.push({
      url: String(input),
      method: init?.method ?? "GET",
      body: typeof init?.body === "string" ? init.body : null,
    });
    return new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
  return { runtime: new CratestackRuntime("http://example.invalid", { fetch: fetchFn }), sent };
}

describe("decode: a BigInt field is an exact bigint", () => {
  it.each(BOUNDARY)("%s survives LedgerApi.get exactly", async (wire) => {
    const { runtime } = runtimeReturning({ id: "l1", amountE8: wire });
    const ledger = await new LedgerApi(runtime).get("l1");

    expect(typeof ledger.amountE8).toBe("bigint");
    expect(ledger.amountE8).toBe(BigInt(wire));
    // The point of the type: the same digits as a double are NOT the value.
    if (wire === "9007199254740993") {
      expect(Number(wire)).toBe(9007199254740992);
      expect(ledger.amountE8).not.toBe(BigInt(Number(wire)));
    }
  });

  it("revives 0 and -1, and leaves a null optional alone", async () => {
    const { runtime } = runtimeReturning({ amountE8: "0", feeE8: null, tiers: ["-1", "0"] });
    const ledger = await new LedgerApi(runtime).get("l1");
    expect(ledger.amountE8).toBe(0n);
    expect(ledger.feeE8).toBeNull();
    expect(ledger.tiers).toEqual([-1n, 0n]);
  });

  it("revives each item of a BigInt[] and leaves an empty one alone", async () => {
    const full = await new LedgerApi(
      runtimeReturning({ tiers: [...BOUNDARY] }).runtime,
    ).get("l1");
    expect(full.tiers).toEqual(BOUNDARY.map((wire) => BigInt(wire)));

    const empty = await new LedgerApi(runtimeReturning({ tiers: [] }).runtime).get("l1");
    expect(empty.tiers).toEqual([]);
  });

  it("revives through a relation, in both directions, and by type not by name", async () => {
    // `Entry.amountE8` is a String and `Ledger.amountE8` a BigInt: the
    // digits-only String must come back untouched, leading zeros included.
    const entry = await new EntryApi(
      runtimeReturning({
        id: "e1",
        deltaE8: "9223372036854775807",
        amountE8: "00123",
        ledger: { id: "l1", amountE8: "-9223372036854775808", tiers: ["7"] },
      }).runtime,
    ).get("e1");
    expect(entry.deltaE8).toBe(9223372036854775807n);
    expect(entry.amountE8).toBe("00123");
    expect(entry.ledger?.amountE8).toBe(-9223372036854775808n);
    expect(entry.ledger?.tiers).toEqual([7n]);

    const ledger = await new LedgerApi(
      runtimeReturning({
        id: "l1",
        amountE8: "1",
        entries: [
          { id: "e1", deltaE8: "2", amountE8: "007" },
          { id: "e2", deltaE8: "-3", amountE8: "9007199254740993" },
        ],
      }).runtime,
    ).get("l1");
    expect(ledger.entries?.map((e) => e.deltaE8)).toEqual([2n, -3n]);
    expect(ledger.entries?.map((e) => e.amountE8)).toEqual(["007", "9007199254740993"]);
  });

  it("revives a ?fields= projection that carries only the BigInt, and one that omits it", async () => {
    const only = await new LedgerApi(runtimeReturning({ amountE8: "5" }).runtime).get("l1", {
      query: { fields: ["amountE8"] },
    });
    expect(only).toEqual({ amountE8: 5n });

    const without = await new LedgerApi(runtimeReturning({ reference: "r" }).runtime).get("l1", {
      query: { fields: ["reference"] },
    });
    expect(without).toEqual({ reference: "r" });
    expect("amountE8" in without).toBe(false);
  });

  it("revives the items of a Page<Ledger> envelope and leaves the envelope alone", async () => {
    const page = await new LedgerApi(
      runtimeReturning({
        items: [{ amountE8: "9223372036854775807" }, { amountE8: "-1" }],
        totalCount: 2,
        pageInfo: { limit: 2, offset: 0, hasNextPage: false, hasPreviousPage: false },
      }).runtime,
    ).list();
    expect(page.items.map((l) => l.amountE8)).toEqual([9223372036854775807n, -1n]);
    expect(page.totalCount).toBe(2);
  });

  it("a BigInt @id is a bigint on get(id) and travels in the path as its digits", async () => {
    const { runtime, sent } = runtimeReturning({ id: "9223372036854775807", hitsE8: "5" });
    const counter = await new CounterApi(runtime).get(9223372036854775807n);
    expect(sent[0]?.url).toBe("http://example.invalid/api/counters/9223372036854775807");
    expect(counter.id).toBe(9223372036854775807n);
    expect(counter.hitsE8).toBe(5n);
  });
});

describe("decode: anything but a canonical decimal string at a BigInt key throws", () => {
  const ledgerGet = (body: unknown) => new LedgerApi(runtimeReturning(body).runtime).get("l1");

  it("refuses a JSON number, naming the type, the field and what it was", async () => {
    // 2^53 + 1 as a number is already 2^53 by the time it is parsed: the
    // case a silent coercion would turn into a wrong answer.
    await expect(ledgerGet({ amountE8: 9007199254740992 })).rejects.toThrow(TypeError);
    await expect(ledgerGet({ amountE8: 9007199254740992 })).rejects.toThrow(
      /BigInt field Ledger\.amountE8: expected a canonical decimal string, got number 9007199254740992/,
    );
  });

  it("refuses a number in an optional, a list item, a relation and a type", async () => {
    await expect(ledgerGet({ feeE8: 5 })).rejects.toThrow(/Ledger\.feeE8/);
    await expect(ledgerGet({ tiers: ["1", 2] })).rejects.toThrow(/Ledger\.tiers/);
    await expect(ledgerGet({ entries: [{ deltaE8: 3 }] })).rejects.toThrow(/Entry\.deltaE8/);
    const entry = new EntryApi(runtimeReturning({ ledger: { amountE8: 1 } }).runtime).get("e1");
    await expect(entry).rejects.toThrow(/Ledger\.amountE8/);
  });

  it("refuses a bigint and a boolean too, as @cratestack/cbor-node's integers would arrive", () => {
    expect(() => reviveWireFields({ amountE8: 5n }, "Ledger")).toThrow(/got bigint 5/);
    expect(() => reviveWireFields({ amountE8: true }, "Ledger")).toThrow(/got boolean true/);
    expect(() => reviveWireFields({ amountE8: {} }, "Ledger")).toThrow(/got an object/);
  });

  it.each([
    ["a leading plus", "+5"],
    ["leading zeros", "007"],
    ["negative zero", "-0"],
    ["surrounding whitespace", " 1"],
    ["an empty string", ""],
    ["a hex literal", "0x10"],
    ["a fraction", "1.0"],
    ["one past i64::MAX", "9223372036854775808"],
    ["one below i64::MIN", "-9223372036854775809"],
    ["twenty digits", "12345678901234567890"],
  ])("refuses %s (%j) instead of normalising it", (_label, wire) => {
    expect(() => reviveWireFields({ amountE8: wire }, "Ledger")).toThrow(
      /BigInt field Ledger\.amountE8: expected a canonical decimal string/,
    );
  });
});

describe("procedures: a bare BigInt return and a BigInt argument", () => {
  const procedures = (body: unknown) => {
    const { runtime, sent } = runtimeReturning(body);
    return { api: new ProceduresApi(runtime), sent };
  };

  it("revives bigint, bigint | null, bigint[] and a type with a BigInt", async () => {
    expect(await procedures("9223372036854775807").api.balance({ reference: "r" })).toBe(
      9223372036854775807n,
    );
    expect(await procedures(null).api.maybeBalance({ reference: "r" })).toBeNull();
    expect(await procedures("-9223372036854775808").api.maybeBalance({ reference: "r" })).toBe(
      -9223372036854775808n,
    );
    expect(await procedures([...BOUNDARY]).api.history({ reference: "r" })).toEqual(
      BOUNDARY.map((wire) => BigInt(wire)),
    );
    expect(
      await procedures({ grossE8: "9007199254740993", history: ["1", "2"] }).api.totals({ reference: "r" }),
    ).toEqual({ grossE8: 9007199254740993n, history: [1n, 2n] });
  });

  it("revives the items of a Page<Ledger> procedure return", async () => {
    const page = await procedures({
      items: [{ amountE8: "9007199254740993" }],
      totalCount: 1,
      pageInfo: { limit: 1, offset: 0, hasNextPage: false, hasPreviousPage: false },
    }).api.ledgers({ reference: "r" });
    expect(page.items[0]?.amountE8).toBe(9007199254740993n);
  });

  it("refuses a number as a bare return", async () => {
    await expect(procedures(5).api.balance({ reference: "r" })).rejects.toThrow(
      /BigInt field procedure return: expected a canonical decimal string, got number 5/,
    );
    expect(() => reviveWireScalar([1], "bigint")).toThrow(TypeError);
  });

  it.each(BOUNDARY)("sends a bigint argument (%s) as the same string", async (wire) => {
    const { api, sent } = procedures("1");
    await api.scale({ amountE8: BigInt(wire), factorE8: null });
    expect(sent[0]?.method).toBe("POST");
    expect(sent[0]?.body).toBe(`{"amountE8":"${wire}","factorE8":null}`);
  });
});

describe("encode: a bigint becomes its canonical string on every REST path", () => {
  it.each(BOUNDARY)("create body carries %s as a string, not a number", async (wire) => {
    const { runtime, sent } = runtimeReturning({ id: "l1", amountE8: wire });
    const created = await new LedgerApi(runtime).create({
      reference: "r",
      amountE8: BigInt(wire),
      feeE8: null,
      tiers: [1n, BigInt(wire)],
    });
    expect(sent[0]?.body).toBe(
      `{"reference":"r","amountE8":"${wire}","feeE8":null,"tiers":["1","${wire}"]}`,
    );
    // Decode then re-encode: the value comes back as the same digits.
    const again = runtimeReturning({});
    await new LedgerApi(again.runtime).update("l1", { amountE8: created.amountE8! });
    expect(JSON.parse(again.sent[0]!.body!)).toEqual({ amountE8: wire });
  });

  it("a BigIntFilter's operands encode too", () => {
    const where = { amountE8: { gt: 5n, in: [1n, 9223372036854775807n], isNull: false } };
    expect(JSON.stringify(encodeWireFields(where))).toBe(
      '{"amountE8":{"gt":"5","in":["1","9223372036854775807"],"isNull":false}}',
    );
  });

  it("an object-valued query entry holding a bigint does not throw", async () => {
    // The REST query path that `JSON.stringify`s an object itself
    // (`computedParams` is the generated caller): a bigint inside it threw
    // "Do not know how to serialize a BigInt" before the conversion lived
    // in the runtime.
    const { runtime, sent } = runtimeReturning({});
    await runtime.get("/anything", {
      query: { nested: { deep: [{ n: 9223372036854775807n }] }, scalar: 7n, list: [1n, 2n] },
    });
    const url = new URL(sent[0]!.url);
    expect(JSON.parse(url.searchParams.get("nested")!)).toEqual({ deep: [{ n: "9223372036854775807" }] });
    expect(url.searchParams.get("scalar")).toBe("7");
    expect(url.searchParams.getAll("list")).toEqual(["1", "2"]);
  });

  it("the JSON pre-walk converts bigint and Decimal, and still turns bytes into an array", () => {
    const walked = encodeBinaryAsJson({
      big: 3n,
      dec: new Decimal("1.5"),
      bytes: Uint8Array.of(1, 2),
      nested: [{ big: -4n }],
    });
    expect(JSON.stringify(walked)).toBe('{"big":"3","dec":"1.5","bytes":[1,2],"nested":[{"big":"-4"}]}');
  });

  it("encodeWireFields leaves bytes alone (the native codec wants the real Uint8Array)", () => {
    const bytes = Uint8Array.of(1, 2);
    const walked = encodeWireFields({ big: 3n, bytes }) as { big: unknown; bytes: unknown };
    expect(walked.big).toBe("3");
    expect(walked.bytes).toBe(bytes);
  });
});
