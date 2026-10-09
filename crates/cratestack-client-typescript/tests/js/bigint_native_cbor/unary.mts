// A generated RPC client on the real `@cratestack/cbor`: a `BigInt` decodes
// from a CBOR text string to an exact `bigint`, and a `bigint` goes back out
// as that same text string, byte for byte, on every unary path.
import { CounterApi, LedgerApi, ProceduresApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";
import { VALUES, cborResponse, check, encoded, hex, includesBytes, realCodec, textStringBytes } from "./common.mjs";

const codec = await realCodec();

function capturing(respond: () => Promise<Response>) {
  const sent: Uint8Array[] = [];
  const fetchFn: typeof fetch = async (_url, init) => {
    check(
      init?.body instanceof Uint8Array,
      `the native codec should hand fetch() a Uint8Array body, got ${typeof init?.body}`,
    );
    sent.push(init.body);
    return respond();
  };
  return { runtime: new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn }), sent };
}

for (const wire of VALUES) {
  const big = BigInt(wire);
  const text = await textStringBytes(wire);

  // update(): decode the response, and compare the request to a plain-string one.
  {
    const { runtime, sent } = capturing(() =>
      cborResponse({ id: "l1", reference: "r", amountE8: wire, feeE8: null, tiers: [wire, wire] }),
    );
    const ledger = await new LedgerApi(runtime).update("l1", { amountE8: big });
    check(ledger.amountE8 === big, `${wire}: amountE8 decoded to ${String(ledger.amountE8)}`);
    check(ledger.feeE8 === null, `${wire}: a null optional must stay null`);
    check(
      ledger.tiers?.length === 2 && ledger.tiers.every((tier) => tier === big),
      `${wire}: tiers decoded to ${String(ledger.tiers)}`,
    );

    const request = codec.decode(sent[0]!) as { id: string; patch: { amountE8: unknown } };
    check(
      typeof request.patch.amountE8 === "string" && request.patch.amountE8 === wire,
      `${wire}: the request carried ${typeof request.patch.amountE8} ${String(request.patch.amountE8)}, want the string`,
    );
    check(includesBytes(sent[0]!, text), `${wire}: request bytes lack the text string ${hex(text)}`);
    const expected = await encoded({ id: "l1", patch: { amountE8: wire } });
    check(hex(sent[0]!) === hex(expected), `${wire}: ${hex(sent[0]!)} != the plain-string request ${hex(expected)}`);

    // Decode, then send the decoded value straight back: the same bytes.
    const again = capturing(() => cborResponse({}));
    await new LedgerApi(again.runtime).update("l1", { amountE8: ledger.amountE8! });
    check(hex(again.sent[0]!) === hex(expected), `${wire}: the re-encoded request differs from the original`);
  }

  // create(): a required, a null optional and a list, all in one body.
  {
    const { runtime, sent } = capturing(() => cborResponse({ id: "l1" }));
    await new LedgerApi(runtime).create({ reference: "r", amountE8: big, feeE8: null, tiers: [big, 1n] });
    const request = codec.decode(sent[0]!) as Record<string, unknown>;
    check(request.amountE8 === wire && request.feeE8 === null, `${wire}: create body ${JSON.stringify(request)}`);
    check(
      Array.isArray(request.tiers) && request.tiers[0] === wire && request.tiers[1] === "1",
      `${wire}: create tiers ${JSON.stringify(request.tiers)}`,
    );
  }

  // A BigInt @id: get(id: bigint) sends the digits, and the row decodes to bigints.
  {
    const { runtime, sent } = capturing(() => cborResponse({ id: wire, hitsE8: wire }));
    const counter = await new CounterApi(runtime).get(big);
    const request = codec.decode(sent[0]!) as { id: unknown };
    check(request.id === wire, `${wire}: get(id) sent ${typeof request.id} ${String(request.id)}`);
    check(counter.id === big && counter.hitsE8 === big, `${wire}: counter decoded to ${String(counter.id)}`);
  }

  // A procedure: bigint arguments out, a bare bigint return back.
  {
    const { runtime, sent } = capturing(() => cborResponse(wire));
    const result = await new ProceduresApi(runtime).scale({ amountE8: big, factorE8: big });
    const request = codec.decode(sent[0]!) as Record<string, unknown>;
    check(request.amountE8 === wire && request.factorE8 === wire, `${wire}: scale args ${JSON.stringify(request)}`);
    check(result === big, `${wire}: scale() returned ${String(result)}`);
  }
}

// The pinned bytes: `{"amountE8": <value>}` as CBOR, written out as hex so an
// encoder regression cannot move the expectation with it. They are the same
// literals `crates/cratestack-codec-cbor/tests/bigint_codec.rs` asserts the Rust
// codec against: map(1), text(8) "amountE8", then the value as a major type 3
// TEXT string (0x60 | length, then the ASCII digits), never `1b7fffffffffffffff`.
const PINNED: Array<[bigint, string]> = [
  [9223372036854775807n, "a168616d6f756e7445387339323233333732303336383534373735383037"],
  [-9223372036854775808n, "a168616d6f756e744538742d39323233333732303336383534373735383038"],
  [9007199254740993n, "a168616d6f756e7445387039303037313939323534373430393933"],
  [0n, "a168616d6f756e7445386130"],
  [-1n, "a168616d6f756e744538622d31"],
];

for (const [big, pinned] of PINNED) {
  // Out: what the generated client puts on the wire for a bigint field.
  const { runtime, sent } = capturing(() => cborResponse({ id: "l1" }));
  await runtime.call("model.Ledger.create", { amountE8: big });
  check(hex(sent[0]!) === pinned, `${big}: client sent ${hex(sent[0]!)}, pinned ${pinned}`);

  // In: those exact bytes, as a server writes them, decode to the exact bigint.
  const response = () => Promise.resolve(new Response(Buffer.from(pinned, "hex"), { status: 200 }));
  const decoded = await new LedgerApi(capturing(response).runtime).get("l1");
  check(decoded.amountE8 === big, `${big}: pinned bytes decoded to ${String(decoded.amountE8)}`);
}

console.log("NATIVE_CBOR_BIGINT_UNARY_OK");
