// ADR 0019, RPC: a `BigInt` inside `computedParams`. RPC carries the params
// as the raw JSON TEXT of the object (`RpcListInput::computed_params`), and
// the generated client does that `JSON.stringify` itself, so a `bigint`
// inside it threw before the conversion moved in front of it. Run against the
// package generated from `tests/fixtures/bigint_computed_params_rpc.cstack`
// (`native_cbor: false`).
import { describe, expect, it } from "vitest";
import { QuoteApi } from "./src/client.js";
import { CratestackRpcRuntime } from "./src/runtime.js";

function runtime(bodies: string[]): CratestackRpcRuntime {
  const fetchFn: typeof fetch = async (_input, init) => {
    bodies.push(typeof init?.body === "string" ? init.body : "");
    return new Response(JSON.stringify({ id: "q1", label: "x" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
  return new CratestackRpcRuntime("http://example.invalid", { fetch: fetchFn });
}

describe("RPC computedParams with a bigint", () => {
  it.each(["9223372036854775807", "-9223372036854775808", "9007199254740993"])(
    "get() sends factorE8 %s as a string inside the params text",
    async (wire) => {
      const bodies: string[] = [];
      await new QuoteApi(runtime(bodies)).get("q1", {
        computedParams: { label: { factorE8: BigInt(wire) } },
      });
      const input = JSON.parse(bodies[0]!) as { id: string; computedParams: string };
      expect(typeof input.computedParams).toBe("string");
      expect(JSON.parse(input.computedParams)).toEqual({ label: { factorE8: wire } });
    },
  );

  it("list() does too", async () => {
    const bodies: string[] = [];
    await new QuoteApi(runtime(bodies)).list({ computedParams: { label: { factorE8: 7n } } });
    const input = JSON.parse(bodies[0]!) as { computedParams: string };
    expect(JSON.parse(input.computedParams)).toEqual({ label: { factorE8: "7" } });
  });
});
