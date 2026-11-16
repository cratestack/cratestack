// A generated RPC client's signed subscription open (cratestack#1378): a
// runtime given `subscriptions.seal` opens a subscription the schema marks
// `@frames(unsigned)` through the sealer's `fetch` (a POST of the input's CBOR),
// and refuses everything else before anything is sent. The sealer is a fake
// here; the real one is `@cratestack/cbor`'s `createSealedSubscriptions`,
// tested in `packages/cratestack-cbor`.
import { createCborCodec } from "@cratestack/cbor";
import { describe, expect, it } from "vitest";
import { CratestackClientClient as CratestackClient } from "./src/client.js";
import { CratestackSubscriptionError, type SubscriptionSeal } from "./src/subscriptions.js";

const codec = await createCborCodec();
const frame = (value: object) => codec.encode(value) as Uint8Array;
const concat = (parts: Uint8Array[]) => Uint8Array.from(parts.flatMap((part) => Array.from(part)));

const SEQ = "application/cbor-seq";
const SSE = "text/event-stream";

interface Sent {
  url: string;
  method: string | undefined;
  headers: Headers;
  body: unknown;
}

/** A sealer that records every open and answers it with `answer`. */
function sealer(answer: (sent: Sent) => Response) {
  const targets: { opId: string; ssePath: string }[] = [];
  const sent: Sent[] = [];
  const seal: SubscriptionSeal = {
    fetchFor(target) {
      targets.push(target);
      return (async (input: RequestInfo | URL, init?: RequestInit) => {
        const open: Sent = {
          url: String(input),
          method: init?.method,
          headers: new Headers(init?.headers),
          body: init?.body instanceof Uint8Array ? codec.decode(init.body) : init?.body,
        };
        sent.push(open);
        return answer(open);
      }) as typeof fetch;
    },
  };
  return { seal, targets, sent };
}

const streamOf = (type: string, body: Uint8Array) =>
  new Response(body, { headers: { "content-type": type, "cratestack-frames": "unsigned" } });

const ticks = concat([
  frame({ id: 1, ready: {} }),
  frame({ id: 2, next: { body: "one" } }),
  frame({ id: 3, end: {} }),
]);

const sseBody = new TextEncoder().encode(
  'event: ready\ndata: {"id":1,"ready":{}}\n\nevent: message\ndata: {"id":2,"next":{"body":"one"}}\n\n',
);

/** A client whose plain `fetch` fails the test if it is ever used. */
function client(seal: SubscriptionSeal | undefined, transport?: "sse" | "cbor-seq" | "websocket" | "webtransport") {
  const plain: string[] = [];
  const fetch = (async (input: RequestInfo | URL) => {
    plain.push(String(input));
    return new Response("{}", { status: 500 });
  }) as typeof globalThis.fetch;
  const subscriptions = { ...(seal === undefined ? {} : { seal }), ...(transport === undefined ? {} : { transport }) };
  return { client: new CratestackClient("http://api.test", { fetch, subscriptions }), plain };
}

async function collect(iterable: AsyncIterable<unknown>) {
  const items: unknown[] = [];
  try {
    for await (const item of iterable) items.push(item);
  } catch (error) {
    return { items, error };
  }
  return { items, error: undefined };
}

describe("a signed open", () => {
  it("is a POST of the input's CBOR to the bare route, over cbor-seq by default", async () => {
    const { seal, targets, sent } = sealer(() => streamOf(SEQ, ticks));
    const { client: c, plain } = client(seal);
    const { items, error } = await collect(c.procedures.pulse({ args: { roomId: "r1" } }));
    expect(error).toBeUndefined();
    expect(items).toEqual([{ body: "one" }]);
    expect(targets).toEqual([{ opId: "procedure.pulse", ssePath: "/rpc/subscribe/procedure.pulse" }]);
    expect(sent).toHaveLength(1);
    expect(sent[0]?.url).toBe("http://api.test/api/rpc/subscribe/procedure.pulse");
    expect(sent[0]?.method).toBe("POST");
    expect(sent[0]?.headers.get("accept")).toBe(SEQ);
    expect(sent[0]?.headers.get("content-type")).toBe("application/cbor");
    expect(sent[0]?.body).toEqual({ args: { roomId: "r1" } });
    expect(plain).toEqual([]);
  });

  it("is a POST over SSE when SSE is chosen", async () => {
    const { seal, sent } = sealer(() => streamOf(SSE, sseBody));
    const { client: c, plain } = client(seal, "sse");
    const { items, error } = await collect(c.procedures.pulse({ args: { roomId: "r1" } }));
    expect(error).toBeUndefined();
    expect(items).toEqual([{ body: "one" }]);
    expect(sent[0]?.method).toBe("POST");
    expect(sent[0]?.headers.get("accept")).toBe(SSE);
    expect(sent[0]?.body).toEqual({ args: { roomId: "r1" } });
    expect(plain).toEqual([]);
  });

  it("opens a sealed refusal's CBOR body as the error it carries", async () => {
    const { seal } = sealer(
      () =>
        new Response(frame({ code: "permission_denied", message: "no" }), {
          status: 403,
          headers: { "content-type": "application/cbor" },
        }),
    );
    const { error } = await collect(client(seal).client.procedures.pulse({ args: { roomId: "r1" } }));
    expect(error).toBeInstanceOf(CratestackSubscriptionError);
    expect((error as CratestackSubscriptionError).code).toBe("permission_denied");
    expect((error as CratestackSubscriptionError).status).toBe(403);
  });

  it("lets the sealer's own refusal through", async () => {
    const refusing: SubscriptionSeal = {
      fetchFor: () => {
        throw new Error("UnsignedFramesRefused stand-in");
      },
    };
    const { error } = await collect(client(refusing).client.procedures.pulse({ args: { roomId: "r1" } }));
    expect((error as Error).message).toBe("UnsignedFramesRefused stand-in");
  });
});

describe("a runtime with a sealer refuses locally, sending nothing", () => {
  const never = () => {
    throw new Error("nothing may be sent");
  };

  it("a subscription the schema does not mark", async () => {
    const { seal, targets } = sealer(never);
    const { client: c, plain } = client(seal);
    const { items, error } = await collect(c.procedures.lines({ args: { roomId: "r1" } }));
    expect(items).toEqual([]);
    expect(error).toBeInstanceOf(CratestackSubscriptionError);
    expect((error as CratestackSubscriptionError).code).toBe("streams_unsupported");
    expect(targets).toEqual([]);
    expect(plain).toEqual([]);
  });

  it("a socket binding, marked or not", async () => {
    for (const transport of ["websocket", "webtransport"] as const) {
      const { seal, targets } = sealer(never);
      const { client: c, plain } = client(seal, transport);
      const { error } = await collect(c.procedures.pulse({ args: { roomId: "r1" } }));
      expect((error as CratestackSubscriptionError).code).toBe("streams_unsupported");
      expect(targets).toEqual([]);
      expect(plain).toEqual([]);
    }
  });
});

describe("a runtime without a sealer", () => {
  it("opens a marked subscription as it always did: a plain GET of the query", async () => {
    const plain: { url: string; accept: string | null }[] = [];
    const fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      plain.push({ url: String(input), accept: new Headers(init?.headers).get("accept") });
      return new Response(ticks, { headers: { "content-type": SEQ } });
    }) as typeof globalThis.fetch;
    const c = new CratestackClient("http://api.test", { fetch, subscriptions: { transport: "cbor-seq" } });
    const { items, error } = await collect(c.procedures.pulse({ args: { roomId: "r1" } }));
    expect(error).toBeUndefined();
    expect(items).toEqual([{ body: "one" }]);
    expect(plain[0]?.url).toContain("/rpc/subscribe/procedure.pulse?");
    expect(plain[0]?.accept).toBe(SEQ);
  });
});
