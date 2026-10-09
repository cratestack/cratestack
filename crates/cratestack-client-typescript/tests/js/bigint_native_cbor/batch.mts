// The batch paths on the real `@cratestack/cbor`: an explicit
// `runtime.batch()`, and `@cratestack/link-batch`, which never runs the
// generated `terminalLink` and encodes a batch of RAW inputs with the
// request's own codec. The wrapper around the native codec is what converts
// a `bigint` there; without it the real codec writes a CBOR integer.
import { CounterApi, LedgerApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";
import { createBatchLink } from "./link-batch/index.js";
import { VALUES, cborResponse, check, hex, includesBytes, realCodec, textStringBytes } from "./common.mjs";

const codec = await realCodec();

type Frame = { id: number; op: string; input: Record<string, unknown> };

// 1. runtime.batch() with a raw bigint input.
{
  const wire = VALUES[0];
  let sent: Uint8Array | undefined;
  const fetchFn: typeof fetch = async (_url, init) => {
    sent = init?.body as Uint8Array;
    return cborResponse([{ id: 1, output: null }]);
  };
  const runtime = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn });
  await runtime.batch([
    { id: 1, op: "model.Ledger.update", input: { id: "l1", patch: { amountE8: BigInt(wire) } } },
  ] as never);
  const frames = codec.decode(sent!) as Frame[];
  const patch = frames[0]!.input.patch as { amountE8: unknown };
  check(patch.amountE8 === wire, `batch frame carried ${typeof patch.amountE8} ${String(patch.amountE8)}`);
  check(includesBytes(sent!, await textStringBytes(wire)), "batch bytes lack the major-type-3 text string");
}

// 2. @cratestack/link-batch, every boundary value in one flush.
for (const wire of VALUES) {
  const big = BigInt(wire);
  const bodies: Uint8Array[] = [];
  const fetchFn: typeof fetch = async (_url, init) => {
    bodies.push(init?.body as Uint8Array);
    const frames = codec.decode(init?.body as Uint8Array) as Frame[];
    return cborResponse(
      frames.map((frame) => ({
        id: frame.id,
        output: frame.op === "model.Ledger.update" ? { id: "l1", amountE8: wire } : { id: wire, hitsE8: wire },
      })),
    );
  };
  const runtime = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn, links: [createBatchLink()] });
  const [ledger, counter] = await Promise.all([
    new LedgerApi(runtime).update("l1", { amountE8: big }),
    new CounterApi(runtime).get(big),
  ]);

  check(bodies.length === 1, `${wire}: expected one batched request, got ${bodies.length}`);
  const frames = codec.decode(bodies[0]!) as Frame[];
  check(frames.length === 2, `${wire}: expected two frames, got ${frames.length}`);
  const patch = frames[0]!.input.patch as { amountE8: unknown };
  check(patch.amountE8 === wire, `${wire}: link-batch update frame carried ${typeof patch.amountE8}`);
  check(frames[1]!.input.id === wire, `${wire}: link-batch get frame carried ${typeof frames[1]!.input.id}`);
  check(
    includesBytes(bodies[0]!, await textStringBytes(wire)),
    `${wire}: link-batch bytes lack the text string ${hex(await textStringBytes(wire))}`,
  );
  check(ledger.amountE8 === big && counter.id === big && counter.hitsE8 === big, `${wire}: batched results`);
}

// 3. Two runtimes sharing the native codec hand link-batch ONE codec
//    reference (it partitions by codec identity): their calls still collapse
//    into a single request.
{
  const wire = VALUES[2];
  const bodies: Uint8Array[] = [];
  const fetchFn: typeof fetch = async (_url, init) => {
    bodies.push(init?.body as Uint8Array);
    const frames = codec.decode(init?.body as Uint8Array) as Frame[];
    return cborResponse(frames.map((frame) => ({ id: frame.id, output: { id: wire, hitsE8: wire } })));
  };
  const link = createBatchLink();
  const first = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn, links: [link] });
  const second = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn, links: [link] });
  await Promise.all([new CounterApi(first).get(1n), new CounterApi(second).get(2n)]);
  check(bodies.length === 1, `two runtimes on one native codec should share a batch, got ${bodies.length} requests`);
}

console.log("NATIVE_CBOR_BIGINT_BATCH_OK");
