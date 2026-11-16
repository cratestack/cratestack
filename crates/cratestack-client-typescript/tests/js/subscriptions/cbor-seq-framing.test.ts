// The `"cbor-seq"` transport's framing (cratestack#1001): frames split at
// every byte, a body that stops short, the per-frame cap, a frame that never
// ends.
import { describe, expect, it } from "vitest";
import { CratestackEventTooLargeError, CratestackSubscriptionError } from "./src/subscriptions.js";
import { KEEPALIVE, client, collect, concat, frame, served } from "./cbor-seq-support.js";

describe("cbor-seq", () => {
  it("reads frames split at every byte position", async () => {
    const wire = concat([
      frame({ id: 1, ready: {} }),
      KEEPALIVE,
      frame({ id: 2, next: { body: "café" } }),
      frame({ id: 3, next: { body: "b" } }),
      frame({ id: 4, end: {} }),
    ]);
    for (let at = 1; at < wire.length; at++) {
      const { fetch } = served([wire.slice(0, at), wire.slice(at)]);
      const { items, error } = await collect(client(fetch).procedures.lines({ args: { roomId: "r1" } }));
      expect(error, `split at byte ${at}`).toBeUndefined();
      expect(items, `split at byte ${at}`).toEqual([{ body: "café" }, { body: "b" }]);
    }
  });

  it("throws, not a quiet end, when the body stops inside a frame or between frames", async () => {
    const whole = concat([frame({ id: 1, ready: {} }), frame({ id: 2, next: { body: "x".repeat(40) } })]);
    const cut = await collect(client(served([whole.slice(0, whole.length - 5)]).fetch).procedures.lines({ args: { roomId: "r1" } }));
    expect(cut.error).toBeInstanceOf(CratestackSubscriptionError);
    expect((cut.error as Error).message).toContain("middle of a frame");
    const stopped = await collect(client(served([whole]).fetch).procedures.lines({ args: { roomId: "r1" } }));
    expect(stopped.items).toHaveLength(1);
    expect((stopped.error as Error).message).toContain("without an `end` or `err` frame");
  });

  it("caps one frame, exactly at the cap and one byte over, and cancels the connection", async () => {
    const big = frame({ id: 2, next: { body: "x".repeat(300) } });
    const ready = frame({ id: 1, ready: {} });
    const atCap = await collect(
      client(served([concat([ready, big, frame({ id: 3, end: {} })])]).fetch, big.length).procedures.lines({ args: { roomId: "r1" } }),
    );
    expect(atCap.error).toBeUndefined();
    expect(atCap.items).toHaveLength(1);

    const over = served([concat([ready, big, frame({ id: 3, next: { body: "after" } })])], "idle");
    const refused = await collect(client(over.fetch, big.length - 1).procedures.lines({ args: { roomId: "r1" } }));
    expect(refused.error).toBeInstanceOf(CratestackEventTooLargeError);
    expect((refused.error as CratestackEventTooLargeError).limit).toBe(big.length - 1);
    expect(refused.items).toEqual([]);
    expect(over.state.cancelled).toBe(true);
  });

  it("cuts off a frame that never ends", async () => {
    // A byte string declaring 4 GiB, then padding forever.
    const head = new Uint8Array([0x5a, 0xff, 0xff, 0xff, 0xff]);
    const chunks = [frame({ id: 1, ready: {} }), head, ...Array.from({ length: 50 }, () => new Uint8Array(100).fill(7))];
    const { fetch, state } = served(chunks, "idle");
    const { error } = await collect(client(fetch, 200).procedures.lines({ args: { roomId: "r1" } }));
    expect(error).toBeInstanceOf(CratestackEventTooLargeError);
    expect(state.cancelled).toBe(true);
  });
});
