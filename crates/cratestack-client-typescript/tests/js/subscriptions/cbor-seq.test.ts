// The generated `"cbor-seq"` subscription transport (cratestack#1001) run
// for real against a `fetch` answering with `application/cbor-seq` bytes
// written by `@cratestack/cbor`, the codec of the Rust server's frames.
// Covers Ready gating (a stream must open with `ready`), revival, a model
// event's byte-string `event_id` (against a body captured from the real
// Postgres-backed server), the keep-alive `null`, frames split at every byte,
// `end`, `err`, the frame cap, a body that stops short, and a response that
// is not `application/cbor-seq`.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { createCborCodec } from "@cratestack/cbor";
import { CratestackClientClient as CratestackClient } from "./src/client.js";
import { CratestackEventTooLargeError, CratestackSubscriptionError } from "./src/subscriptions.js";

const codec = await createCborCodec();

/** One frame as a CBOR item. */
function frame(value: object): Uint8Array {
  return codec.encode(value) as Uint8Array;
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

const KEEPALIVE = new Uint8Array([0xf6]);

const event = {
  event_id: "e1",
  model: "Note",
  operation: "Created",
  occurred_at: "2026-11-01T00:00:00Z",
  data: { id: 1, body: "hi", amount: "1.50" },
};

interface Served {
  fetch: typeof fetch;
  calls: { url: string; accept: string | null }[];
  state: { cancelled: boolean };
}

/** A `fetch` answering with `chunks`, then closing, or left open (`idle`). */
function served(
  chunks: Uint8Array[],
  tail: "close" | "idle" = "close",
  contentType = "application/cbor-seq",
): Served {
  const calls: Served["calls"] = [];
  const state = { cancelled: false };
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), accept: new Headers(init?.headers).get("accept") });
    let i = 0;
    const body = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (i < chunks.length) controller.enqueue(chunks[i++]!);
        else if (tail === "close") controller.close();
      },
      cancel() {
        state.cancelled = true;
      },
    });
    return new Response(body, { headers: { "content-type": contentType } });
  }) as typeof fetch;
  return { fetch: fetchFn, calls, state };
}

const client = (fetch: typeof globalThis.fetch, maxEventBytes?: number) =>
  new CratestackClient("http://api.test", {
    fetch,
    subscriptions: maxEventBytes === undefined ? { transport: "cbor-seq" } : { transport: "cbor-seq", maxEventBytes },
  });

async function collect(iterable: AsyncIterable<unknown>): Promise<{ items: unknown[]; error: unknown }> {
  const items: unknown[] = [];
  try {
    for await (const item of iterable) items.push(item);
  } catch (error) {
    return { items, error };
  }
  return { items, error: undefined };
}

describe("cbor-seq", () => {
  it("asks for the media type, gates on ready, revives items and ends on `end`", async () => {
    const { fetch, calls } = served([
      concat([KEEPALIVE, frame({ id: 1, ready: {} }), KEEPALIVE, frame({ id: 2, next: event }), frame({ id: 3, end: {} })]),
    ]);
    let ready = false;
    const { items, error } = await collect(
      client(fetch).notes.subscribe({ operations: ["created"] }, { onReady: () => (ready = true) }),
    );
    expect(error).toBeUndefined();
    expect(ready).toBe(true);
    expect(items).toHaveLength(1);
    expect((items[0] as typeof event).data.amount.toString()).toBe("1.5");
    expect(calls[0]).toEqual({
      url: "http://api.test/api/rpc/subscribe/model.Note.subscribe?operations=created",
      accept: "application/cbor-seq",
    });
  });

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

  it("throws the terminal err frame, and delivers nothing after it", async () => {
    const { fetch } = served([
      concat([
        frame({ id: 1, ready: {} }),
        frame({ id: 2, err: { code: "unavailable", message: "subscription lagged" } }),
        frame({ id: 3, next: { body: "after" } }),
      ]),
    ]);
    const { items, error } = await collect(client(fetch).procedures.lines({ args: { roomId: "r1" } }));
    expect(items).toEqual([]);
    expect(error).toBeInstanceOf(CratestackSubscriptionError);
    expect((error as CratestackSubscriptionError).code).toBe("unavailable");
  });

  it("throws a JSON refusal with its status, before any frame", async () => {
    const fetch = (async () =>
      new Response(JSON.stringify({ code: "permission_denied", message: "no" }), { status: 403 })) as typeof globalThis.fetch;
    const { error } = await collect(client(fetch).procedures.lines({ args: { roomId: "r1" } }));
    expect((error as CratestackSubscriptionError).code).toBe("permission_denied");
    expect((error as CratestackSubscriptionError).status).toBe(403);
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
    const event = items[0] as { event_id: string; data: { id: string; amount: { toString(): string }; body: string } };
    expect(event.event_id).toBe("3143e143-5515-49fa-8e22-9f24d82d0a03");
    expect(event.data.id).toBe("01234567-89ab-cdef-0123-456789abcdef");
    expect(event.data.amount.toString()).toBe("12.5");
    expect(event.data.body).toBe("hello");
    // The captured body stops without an `end`, which is an error.
    expect((error as Error).message).toContain("without an `end` or `err` frame");
  });

  it("refuses a response that is not application/cbor-seq, and cancels its body", async () => {
    const sse = new TextEncoder().encode('event: ready\ndata: {"id":1,"ready":{}}\n\n');
    const wrong = served([sse], "idle", "text/event-stream");
    const refused = await collect(client(wrong.fetch).procedures.lines({ args: { roomId: "r1" } }));
    expect(refused.items).toEqual([]);
    expect(refused.error).toBeInstanceOf(CratestackSubscriptionError);
    expect((refused.error as CratestackSubscriptionError).code).toBe("invalid_response");
    expect((refused.error as Error).message).toContain("text/event-stream");
    expect(wrong.state.cancelled).toBe(true);

    // Parameters and case do not matter.
    const ok = served(
      [concat([frame({ id: 1, ready: {} }), frame({ id: 2, end: {} })])],
      "close",
      "Application/CBOR-Seq; charset=binary",
    );
    expect((await collect(client(ok.fetch).procedures.lines({ args: { roomId: "r1" } }))).error).toBeUndefined();
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
