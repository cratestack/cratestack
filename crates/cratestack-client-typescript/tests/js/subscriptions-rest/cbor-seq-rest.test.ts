// A generated REST client's model feed over `"cbor-seq"` (cratestack#1001),
// fed the body captured from the real Postgres-backed REST server
// (`cratestack-pg/tests/subscription_cbor_seq_shapes.rs`): the REST client
// revives `event_id`, a 16-byte CBOR byte string on this binding, to the
// canonical UUID string, as it does `data.id`. The RPC client's twin is in
// `tests/js/subscriptions/cbor-seq-frames.test.ts`.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { CratestackClientClient } from "./src/client.js";

function wireBody(): Uint8Array {
  const hex = readFileSync(new URL("./cbor-seq-model-feed.hex", import.meta.url), "utf8")
    .split("\n")
    .filter((line) => !line.startsWith("#"))
    .join("")
    .trim();
  return Uint8Array.from(hex.match(/../g)!, (byte) => parseInt(byte, 16));
}

describe("cbor-seq over a REST client", () => {
  it("revives the byte-string event_id and Uuid of a model event", async () => {
    const calls: { url: string; accept: string | null }[] = [];
    const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: String(input), accept: new Headers(init?.headers).get("accept") });
      return new Response(wireBody(), { headers: { "content-type": "application/cbor-seq" } });
    }) as typeof fetch;
    const client = new CratestackClientClient("http://api.test", {
      fetch: fetchFn,
      subscriptions: { transport: "cbor-seq" },
    });
    const items: { event_id: string; data: { id: string; amount: { toFixed(places: number): string }; body: string } }[] = [];
    let error: unknown;
    try {
      for await (const item of client.seqNotes.subscribe()) items.push(item as (typeof items)[number]);
    } catch (caught) {
      error = caught;
    }
    expect(calls).toEqual([{ url: "http://api.test/api/seq_notes/$subscribe", accept: "application/cbor-seq" }]);
    expect(items).toHaveLength(1);
    expect(items[0]!.event_id).toBe("3143e143-5515-49fa-8e22-9f24d82d0a03");
    expect(items[0]!.data.id).toBe("01234567-89ab-cdef-0123-456789abcdef");
    expect(items[0]!.data.amount.toFixed(8)).toBe("12.50000000");
    expect(items[0]!.data.body).toBe("hello");
    // The captured body stops without an `end`, which is an error.
    expect((error as Error).message).toContain("without an `end` or `err` frame");
  });
});
