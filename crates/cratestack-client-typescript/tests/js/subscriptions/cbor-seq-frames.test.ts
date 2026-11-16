// The `"cbor-seq"` transport against a body captured from the real
// Postgres-backed server, and its frame validation (cratestack#1001): a
// byte-string `event_id`, an item that is not a map, a first frame that is
// not `ready`.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { CratestackSubscriptionError } from "./src/subscriptions.js";
import { skipItem } from "./src/cbor-item.js";
import { client, codec, collect, concat, frame, served } from "./cbor-seq-support.js";

describe("cbor-seq", () => {
  it("revives a model event's byte-string event_id and Uuid, as the real server writes them", async () => {
    // Captured from the Postgres-backed server: `event_id` and `data.id` are
    // 16-byte CBOR byte strings, `amount` is text.
    const hex = readFileSync(new URL("./cbor-seq-model-feed.hex", import.meta.url), "utf8")
      .split("\n")
      .filter((line) => !line.startsWith("#"))
      .join("")
      .trim();
    const body = Uint8Array.from(hex.match(/../g)!, (byte) => parseInt(byte, 16));
    const { items, error } = await collect(client(served([body]).fetch).seqNotes.subscribe());
    expect(items).toHaveLength(1);
    const event = items[0] as { event_id: string; data: { id: string; amount: { toFixed(places: number): string }; body: string } };
    expect(event.event_id).toBe("3143e143-5515-49fa-8e22-9f24d82d0a03");
    expect(event.data.id).toBe("01234567-89ab-cdef-0123-456789abcdef");
    // The revived Decimal keeps the value; the wire text (below) is the
    // server's exact `12.50000000`, which `toFixed(8)` writes back.
    expect(event.data.amount.toFixed(8)).toBe("12.50000000");
    expect(event.data.body).toBe("hello");
    const second = skipItem(body, skipItem(body, 0));
    const wire = codec.decode(body.slice(skipItem(body, 0), second)) as { next: { data: { amount: unknown } } };
    expect(wire.next.data.amount).toBe("12.50000000");
    // The captured body stops without an `end`, which is an error.
    expect((error as Error).message).toContain("without an `end` or `err` frame");
  });

  it("turns an item that is not a map into a subscription error", async () => {
    for (const bad of [frame(5 as unknown as object), frame([1, 2] as unknown as object), frame("x" as unknown as object)]) {
      const { fetch } = served([concat([frame({ id: 1, ready: {} }), bad])]);
      const { error } = await collect(client(fetch).procedures.lines({ args: { roomId: "r1" } }));
      expect(error).toBeInstanceOf(CratestackSubscriptionError);
      expect((error as CratestackSubscriptionError).code).toBe("invalid_response");
    }
  });

  it("refuses a stream whose first frame is not `ready`, and yields nothing", async () => {
    for (const first of [frame({ id: 1, next: { body: "early" } }), frame({ id: 1, end: {} })]) {
      const { fetch, state } = served([concat([first, frame({ id: 2, ready: {} })])], "idle");
      const { items, error } = await collect(client(fetch).procedures.lines({ args: { roomId: "r1" } }));
      expect(items).toEqual([]);
      expect(error).toBeInstanceOf(CratestackSubscriptionError);
      expect((error as CratestackSubscriptionError).code).toBe("invalid_response");
      expect((error as Error).message).toContain("`ready`");
      expect(state.cancelled).toBe(true);
    }
    // An `err` first is the server's refusal, not a framing fault.
    const refusal = served([frame({ id: 1, err: { code: "permission_denied", message: "no" } })]);
    const { error } = await collect(client(refusal.fetch).procedures.lines({ args: { roomId: "r1" } }));
    expect((error as CratestackSubscriptionError).code).toBe("permission_denied");
  });
});
