// The generated `"cbor-seq"` subscription transport (cratestack#1001) run
// for real against a `fetch` answering with `application/cbor-seq` bytes
// written by `@cratestack/cbor`, the codec of the Rust server's frames.
// This file: the happy path, the terminal frames and the refusals. Framing
// and caps are in `cbor-seq-framing.test.ts`, the real server body and frame
// validation in `cbor-seq-frames.test.ts`; a missing codec is tested on a REST
// package, in `tests/js/subscriptions-rest/`.
import { describe, expect, it } from "vitest";
import { CratestackSubscriptionError } from "./src/subscriptions.js";
import { KEEPALIVE, client, collect, concat, event, frame, served } from "./cbor-seq-support.js";

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
});
