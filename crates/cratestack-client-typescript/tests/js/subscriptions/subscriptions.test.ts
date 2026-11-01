// The generated subscription surface (slice S6) run for real against
// stand-ins for the server: a `fetch` answering with an SSE body, and a
// `WebSocket` class answering frames. Covers Ready gating, revival, the
// terminal error, socket multiplexing with `cancel`, and `"auto"` falling
// back to SSE when no socket opens but never on a refusal.
import { afterEach, describe, expect, it } from "vitest";
// The class takes the default package name (`cratestack-client`).
import { CratestackClientClient as CratestackClient } from "./src/client.js";
import { CratestackSubscriptionError } from "./src/subscriptions.js";

const encoder = new TextEncoder();

function sse(events: string[]): typeof fetch {
  return (async (input: RequestInfo | URL) => {
    const url = String(input);
    calls.push(url);
    if (url.includes("refused")) {
      return new Response(JSON.stringify({ code: "permission_denied", message: "no" }), { status: 403 });
    }
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const event of events) controller.enqueue(encoder.encode(event));
        controller.close();
      },
    });
    return new Response(body, { headers: { "content-type": "text/event-stream" } });
  }) as typeof fetch;
}

let calls: string[] = [];

afterEach(() => {
  calls = [];
  delete (globalThis as { WebSocket?: unknown }).WebSocket;
});

const event = {
  event_id: "e1",
  model: "Note",
  operation: "Created",
  occurred_at: "2026-11-01T00:00:00Z",
  data: { id: 1, body: "hi", amount: "1.50" },
};

describe("SSE", () => {
  it("resolves after ready, revives items, and ends on its own", async () => {
    const client = new CratestackClient("http://api.test", {
      fetch: sse([
        'event: ready\ndata: {"id":1,"ready":{}}\n\n',
        ": keepalive\n\n",
        `event: message\ndata: ${JSON.stringify({ id: 2, next: event })}\n\n`,
      ]),
      subscriptions: { transport: "sse" },
    });
    let ready = false;
    const items = [];
    for await (const item of client.notes.subscribe({ operations: ["created"] }, { onReady: () => (ready = true) })) {
      items.push(item);
    }
    expect(ready).toBe(true);
    expect(items).toHaveLength(1);
    expect(items[0]!.data.amount.toString()).toBe("1.5");
    expect(calls[0]).toBe("http://api.test/api/rpc/subscribe/model.Note.subscribe?operations=created");
  });

  it("throws the terminal error and the refusal", async () => {
    const client = new CratestackClient("http://api.test", {
      fetch: sse([
        'event: ready\ndata: {"id":1,"ready":{}}\n\n',
        'event: error\ndata: {"id":2,"err":{"code":"unavailable","message":"subscription lagged"}}\n\n',
      ]),
      subscriptions: { transport: "sse" },
    });
    await expect(async () => {
      for await (const _ of client.procedures.lines({ args: { roomId: "r1" } })) {
        // never reached
      }
    }).rejects.toThrow("subscription lagged");
    expect(calls[0]).toContain("/rpc/subscribe/procedure.lines?args=%7B%22roomId%22%3A%22r1%22%7D");
  });
});

/** A server on the other end of the socket: answers Init, then each Request. */
class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  static refuse = false;
  static unreachable = false;
  readyState = 0;
  sent: Record<string, unknown>[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((message: { data: string }) => void) | null = null;
  onclose: ((event: { code: number }) => void) | null = null;

  constructor(readonly url: string, readonly protocol: string) {
    FakeSocket.instances.push(this);
    setTimeout(() => {
      if (FakeSocket.unreachable) return this.onclose?.({ code: 1006 });
      this.readyState = 1;
      this.onopen?.();
    });
  }

  send(text: string): void {
    const frame = JSON.parse(text) as { id: number; init?: unknown; op?: string; cancel?: unknown };
    this.sent.push(frame);
    const reply = (value: object) => setTimeout(() => this.onmessage?.({ data: JSON.stringify(value) }));
    if (frame.init) {
      reply(FakeSocket.refuse ? { id: 0, err: { code: "unauthenticated", message: "bad token" } } : { id: 0, ready: {} });
    } else if (frame.op) {
      reply({ id: frame.id, ready: {} });
      reply({ id: frame.id, next: frame.op === "procedure.lines" ? { body: "over the socket" } : event });
    } else if (frame.cancel) {
      reply({ id: frame.id, end: {} });
    }
  }

  close(): void {}
}

describe("WebSocket", () => {
  afterEach(() => {
    FakeSocket.instances = [];
    FakeSocket.refuse = false;
    FakeSocket.unreachable = false;
  });

  it("multiplexes subscriptions on one shared socket and cancels on break", async () => {
    (globalThis as { WebSocket?: unknown }).WebSocket = FakeSocket;
    const client = new CratestackClient("https://api.test", {
      headers: { authorization: "Bearer 1" },
      subscriptions: { transport: "websocket" },
    });
    const lines = client.procedures.lines({ args: { roomId: "r1" } })[Symbol.asyncIterator]();
    const notes = client.notes.subscribe()[Symbol.asyncIterator]();
    expect((await lines.next()).value).toEqual({ body: "over the socket" });
    expect((await notes.next()).value!.data.body).toBe("hi");
    await lines.return!();
    expect(FakeSocket.instances).toHaveLength(1);
    const socket = FakeSocket.instances[0]!;
    expect(socket.url).toBe("wss://api.test/api/rpc/ws");
    expect(socket.protocol).toBe("cratestack.rpc.v1+json");
    expect(socket.sent[0]).toEqual({ id: 0, init: { headers: { authorization: "Bearer 1" } } });
    expect(socket.sent.some((frame) => frame.cancel !== undefined)).toBe(true);
    await notes.return!();
  });

  it("auto falls back to SSE when no socket opens, but not on a refusal", async () => {
    (globalThis as { WebSocket?: unknown }).WebSocket = FakeSocket;
    FakeSocket.unreachable = true;
    const fetch = sse(['event: ready\ndata: {"id":1,"ready":{}}\n\n', `event: message\ndata: {"id":2,"next":{"body":"over SSE"}}\n\n`]);
    const fallback = new CratestackClient("https://fallback.test", { fetch });
    const items = [];
    for await (const item of fallback.procedures.lines({ args: { roomId: "r1" } })) items.push(item);
    expect(items).toEqual([{ body: "over SSE" }]);

    FakeSocket.unreachable = false;
    FakeSocket.refuse = true;
    const refused = new CratestackClient("https://refused.test", { fetch });
    const iterator = refused.procedures.lines({ args: { roomId: "r1" } })[Symbol.asyncIterator]();
    await expect(iterator.next()).rejects.toBeInstanceOf(CratestackSubscriptionError);
    expect(calls).toHaveLength(1);
  });
});
