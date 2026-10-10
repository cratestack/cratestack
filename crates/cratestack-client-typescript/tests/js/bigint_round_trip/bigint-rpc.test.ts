// RPC-transport counterpart to `bigint.test.ts` (ADR 0019), on the pure
// TypeScript `jsonRpcCodec` (`native_cbor: false`; the real
// `@cratestack/cbor` is covered by `tests/native_cbor_bigint_encode.rs`).
// Run against the package generated from `tests/fixtures/bigint_scalar_rpc.cstack`
// with `packages/cratestack-link-batch/src` copied in beside it as
// `./link-batch`, so the batch path is the repository's real one.
import { describe, expect, it } from "vitest";
import { CounterApi, LedgerApi, ProceduresApi } from "./src/client.js";
import { CratestackRpcRuntime, jsonRpcCodec, type CratestackRpcClientOptions } from "./src/runtime.js";
import { Decimal } from "./src/models.js";
import { createBatchLink } from "./link-batch/index.js";

const BOUNDARY = ["9223372036854775807", "-9223372036854775808", "9007199254740993"] as const;

interface Sent {
  url: string;
  text: string;
}

function runtimeResponding(
  respond: (url: string, body: unknown) => unknown,
  options: CratestackRpcClientOptions = {},
): { runtime: CratestackRpcRuntime; sent: Sent[] } {
  const sent: Sent[] = [];
  const fetchFn: typeof fetch = async (input, init) => {
    const text = typeof init?.body === "string" ? init.body : "";
    sent.push({ url: String(input), text });
    return new Response(JSON.stringify(respond(String(input), JSON.parse(text))), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
  return { runtime: new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn, ...options }), sent };
}

describe("rpc decode", () => {
  it.each(BOUNDARY)("LedgerApi.get revives %s to an exact bigint", async (wire) => {
    const { runtime } = runtimeResponding(() => ({ id: "l1", amountE8: wire, tiers: [wire], feeE8: null }));
    const ledger = await new LedgerApi(runtime).get("l1");
    expect(ledger.amountE8).toBe(BigInt(wire));
    expect(ledger.tiers).toEqual([BigInt(wire)]);
    expect(ledger.feeE8).toBeNull();
  });

  it("revives a Page<Ledger> list, a relation, and a procedure's bare and typed returns", async () => {
    const page = await new LedgerApi(
      runtimeResponding(() => ({ items: [{ amountE8: "-1" }], totalCount: 1, pageInfo: {} })).runtime,
    ).list();
    expect(page.items[0]?.amountE8).toBe(-1n);

    const ledger = await new LedgerApi(
      runtimeResponding(() => ({ entries: [{ deltaE8: "9007199254740993", amountE8: "007" }] })).runtime,
    ).get("l1");
    expect(ledger.entries?.[0]).toEqual({ deltaE8: 9007199254740993n, amountE8: "007" });

    const procedures = (out: unknown) => new ProceduresApi(runtimeResponding(() => out).runtime);
    expect(await procedures("9223372036854775807").balance({ reference: "r" })).toBe(9223372036854775807n);
    expect(await procedures(null).maybeBalance({ reference: "r" })).toBeNull();
    expect(await procedures(["1", "-2"]).history({ reference: "r" })).toEqual([1n, -2n]);
    expect(await procedures({ grossE8: "3", history: ["4"] }).totals({ reference: "r" })).toEqual({
      grossE8: 3n,
      history: [4n],
    });
  });

  it("throws on a number at a BigInt key, naming the field", async () => {
    await expect(
      new LedgerApi(runtimeResponding(() => ({ amountE8: 9007199254740992 })).runtime).get("l1"),
    ).rejects.toThrow(/BigInt field Ledger\.amountE8: expected a canonical decimal string, got number/);
    await expect(
      new ProceduresApi(runtimeResponding(() => 5).runtime).balance({ reference: "r" }),
    ).rejects.toThrow(/procedure return/);
  });
});

describe("rpc encode: unary", () => {
  it.each(BOUNDARY)("create, update and a BigInt @id send %s as a string", async (wire) => {
    const create = runtimeResponding(() => ({ id: "l1" }));
    await new LedgerApi(create.runtime).create({
      reference: "r",
      amountE8: BigInt(wire),
      feeE8: null,
      tiers: [BigInt(wire)],
    });
    expect(create.sent[0]?.text).toBe(
      `{"reference":"r","amountE8":"${wire}","feeE8":null,"tiers":["${wire}"]}`,
    );

    const update = runtimeResponding(() => ({ id: "l1" }));
    await new LedgerApi(update.runtime).update("l1", { amountE8: BigInt(wire) });
    expect(update.sent[0]?.text).toBe(`{"id":"l1","patch":{"amountE8":"${wire}"}}`);

    const get = runtimeResponding(() => ({ id: wire }));
    const counter = await new CounterApi(get.runtime).get(BigInt(wire));
    expect(get.sent[0]?.text).toBe(`{"id":"${wire}"}`);
    expect(counter.id).toBe(BigInt(wire));
  });

  it("sends a bigint procedure argument as a string", async () => {
    const { runtime, sent } = runtimeResponding(() => "1");
    await new ProceduresApi(runtime).scale({ amountE8: 9223372036854775807n, factorE8: null });
    expect(sent[0]?.text).toBe('{"amountE8":"9223372036854775807","factorE8":null}');
  });

  it("the codec itself converts, so a caller that never reaches terminalLink is covered", () => {
    expect(jsonRpcCodec.encode({ n: 5n, nested: [{ n: -9223372036854775808n }] })).toBe(
      '{"n":"5","nested":[{"n":"-9223372036854775808"}]}',
    );
    expect(jsonRpcCodec.encode({ d: new Decimal("1.5"), b: Uint8Array.of(1) })).toBe('{"d":"1.5","b":[1]}');
  });
});

describe("rpc encode: batch and stream", () => {
  it("runtime.batch() converts the bigint inside each frame's input", async () => {
    const { runtime, sent } = runtimeResponding(() => [{ id: 1, output: null }]);
    await runtime.batch([
      { id: 1, op: "model.Ledger.update", input: { id: "l1", patch: { amountE8: 9007199254740993n } } },
    ] as never);
    expect(sent[0]?.url).toBe("http://example.invalid/api/rpc/batch");
    expect(JSON.parse(sent[0]!.text)).toEqual([
      { id: 1, op: "model.Ledger.update", input: { id: "l1", patch: { amountE8: "9007199254740993" } } },
    ]);
  });

  it("stream() converts a bigint input", async () => {
    const { runtime, sent } = runtimeResponding(() => ["1", "2"]);
    const items: unknown[] = [];
    for await (const item of runtime.stream("procedure.history", { reference: "r", after: 5n })) {
      items.push(item);
    }
    expect(sent[0]?.text).toBe('{"reference":"r","after":"5"}');
    expect(items).toEqual(["1", "2"]);
  });

  it("@cratestack/link-batch encodes the RAW inputs it queues, bigint included", async () => {
    // The batch link never runs `terminalLink`: it encodes a batch of raw
    // inputs with `request.codec` itself, so a conversion that lived only
    // in `terminalLink` would be skipped by exactly this path.
    const { runtime, sent } = runtimeResponding(
      (_url, body) =>
        (body as Array<{ id: number }>).map((frame) => ({
          id: frame.id,
          output: frame.id === 0 ? { id: "l1", amountE8: "9223372036854775807" } : { id: "5", hitsE8: "6" },
        })),
      { links: [createBatchLink()] },
    );
    const [ledger, counter] = await Promise.all([
      new LedgerApi(runtime).update("l1", { amountE8: 9223372036854775807n }),
      new CounterApi(runtime).get(5n),
    ]);

    expect(sent).toHaveLength(1);
    expect(sent[0]?.url).toBe("http://example.invalid/api/rpc/batch");
    expect(JSON.parse(sent[0]!.text)).toEqual([
      { id: 0, op: "model.Ledger.update", input: { id: "l1", patch: { amountE8: "9223372036854775807" } } },
      { id: 1, op: "model.Counter.get", input: { id: "5" } },
    ]);
    // ...and the responses it fans back out are revived as usual.
    expect(ledger.amountE8).toBe(9223372036854775807n);
    expect(counter).toEqual({ id: 5n, hitsE8: 6n });
  });
});
