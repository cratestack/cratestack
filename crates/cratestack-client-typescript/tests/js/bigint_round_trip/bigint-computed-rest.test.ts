// ADR 0019, REST: a `BigInt` inside `computedParams`. REST serialises an
// object-valued query entry with `JSON.stringify` inside the runtime, which
// throws on a `bigint`; the runtime converts first. Run against the package
// generated from `tests/fixtures/bigint_computed_params.cstack`.
import { describe, expect, it } from "vitest";
import { QuoteApi } from "./src/client.js";
import { CratestackRuntime } from "./src/runtime.js";

function runtime(urls: string[]): CratestackRuntime {
  const fetchFn: typeof fetch = async (input) => {
    urls.push(String(input));
    return new Response(JSON.stringify({ id: "q1", label: "x" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
  return new CratestackRuntime("http://example.invalid", { fetch: fetchFn });
}

describe("REST computedParams with a bigint", () => {
  it.each(["9223372036854775807", "-9223372036854775808", "9007199254740993"])(
    "get() sends factorE8 %s as a string inside the JSON query value",
    async (wire) => {
      const urls: string[] = [];
      await new QuoteApi(runtime(urls)).get("q1", {
        query: { computedParams: { label: { factorE8: BigInt(wire) } } },
      });
      const sent = new URL(urls[0]!).searchParams.get("computedParams");
      expect(JSON.parse(sent!)).toEqual({ label: { factorE8: wire } });
    },
  );

  it("list() does too", async () => {
    const urls: string[] = [];
    await new QuoteApi(runtime(urls)).list({
      query: { computedParams: { label: { factorE8: 7n } } },
    });
    expect(JSON.parse(new URL(urls[0]!).searchParams.get("computedParams")!)).toEqual({
      label: { factorE8: "7" },
    });
  });
});
