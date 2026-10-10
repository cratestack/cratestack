// A server from before the cutover sends a `BigInt` field as a CBOR INTEGER.
// `@cratestack/cbor` decodes that to a `number` (up to 2^53 - 1) or a `bigint`
// (above), neither of which may be read as the value: the generated client
// must throw, naming the field.
import { LedgerApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";
import { cborResponse, check } from "./common.mjs";

async function refusal(amountE8: number | bigint): Promise<string> {
  const fetchFn: typeof fetch = async () => cborResponse({ id: "l1", amountE8 });
  const runtime = new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn });
  try {
    await new LedgerApi(runtime).update("l1", { reference: "r" });
  } catch (error) {
    check(error instanceof TypeError, `expected a TypeError, got ${String(error)}`);
    return error.message;
  }
  throw new Error(`a CBOR integer ${String(amountE8)} at a BigInt key was accepted`);
}

const small = await refusal(5);
check(
  /BigInt field Ledger\.amountE8: expected a canonical decimal string, got number 5/.test(small),
  `unexpected message for a small CBOR integer: ${small}`,
);
const large = await refusal(4611686018427387904n);
check(
  /BigInt field Ledger\.amountE8: expected a canonical decimal string, got bigint 4611686018427387904/.test(large),
  `unexpected message for a large CBOR integer: ${large}`,
);

console.log("NATIVE_CBOR_BIGINT_DECODE_REFUSAL_OK");
