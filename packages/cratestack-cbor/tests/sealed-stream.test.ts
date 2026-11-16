// A signed subscription open at the `fetch` boundary (cratestack#1378): the
// open is sealed, and the stream that answers must be the unsigned one that
// was asked for. A fake `CoseClient` here, so these run in every CI lane.
import {
  type CoseCallBinding,
  type CoseClient,
  CoseFailure,
  type CoseOpened,
} from "@cratestack/ts-types";
import { describe, expect, it } from "vitest";
import {
  createSealedFetch,
  createSealedSubscriptions,
  SealRequired,
  UnsignedFramesRefused,
} from "../src/sealed.js";
import { UNSIGNED_REFUSAL } from "../src/sealed-stream.js";

const MEDIA = 'application/cose; cose-type="cose-sign1"';
const SHA = "ab".repeat(32);
const SEQ = "application/cbor-seq";
const SSE = "text/event-stream";
const TARGET = { opId: "procedure.hinted", ssePath: "/rpc/subscribe/procedure.hinted" };

interface Seen {
  sealed: { payload: Uint8Array; binding: CoseCallBinding }[];
  opened: number;
}

function fakeCose(seen: Seen): CoseClient {
  return {
    mode: "sign1",
    mediaType: MEDIA,
    kid: new Uint8Array(8),
    canonicalQuery: (raw) => raw,
    contractHeader: () => "CONTRACT-SEL",
    selectorHeaders: () => ({}),
    async sealRequest(payload, binding) {
      seen.sealed.push({ payload, binding });
      return new Uint8Array([0xd2, 1, 2, 3]);
    },
    async openResponse(): Promise<CoseOpened> {
      seen.opened++;
      return {
        payload: new TextEncoder().encode('{"code":"permission_denied"}'),
        payloadType: "application/json",
        kid: new Uint8Array(8),
        alg: "ed25519",
        thumbprint: new Uint8Array(32),
      };
    },
  };
}

const newSeen = (): Seen => ({ sealed: [], opened: 0 });

interface Call {
  url: string;
  init: RequestInit;
}

/** `answer` is what the server sends back to the open. */
function open(answer: () => Response, cose = fakeCose(newSeen())) {
  const calls: Call[] = [];
  const underlying = (async (url: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(url), init: init ?? {} });
    return answer();
  }) as typeof fetch;
  const fetchFn = createSealedSubscriptions({ cose, contracts: { [TARGET.opId]: SHA } }).fetchFor(
    TARGET,
    underlying,
  );
  const send = (stream = SEQ) =>
    fetchFn("https://x/rpc/subscribe/procedure.hinted", {
      method: "POST",
      headers: { accept: stream, "content-type": "application/cbor" },
      body: new Uint8Array([0xa1, 1, 2]),
    });
  return { send, calls };
}

const stream = (headers: Record<string, string>) =>
  new Response("frames", { status: 200, headers });

describe("createSealedSubscriptions", () => {
  it("seals the open as a POST of the subscribe route and lets the unsigned stream through", async () => {
    const seen = newSeen();
    const marked = () => stream({ "content-type": SEQ, "cratestack-frames": "unsigned" });
    const { send, calls } = open(marked, fakeCose(seen));
    const response = await send();
    expect(await response.text()).toBe("frames");
    expect(seen.sealed[0]?.binding).toMatchObject({
      method: "POST",
      route: "subscribe/procedure.hinted",
      pathParams: [],
      payloadType: "application/cbor",
    });
    // The stream is not a payload type: nothing is bound as the accept.
    expect(seen.sealed[0]?.binding.payloadAccept).toBeUndefined();
    expect(Array.from(seen.sealed[0]?.payload ?? [])).toEqual([0xa1, 1, 2]);
    const headers = new Headers(calls[0]?.init.headers);
    expect(headers.get("content-type")).toBe(MEDIA);
    expect(headers.get("accept")).toBe(`${SEQ}, ${MEDIA}`);
    expect(headers.get("cratestack-contract")).toBe("CONTRACT-SEL");
    expect(calls[0]?.init.method).toBe("POST");
    expect(Array.from(calls[0]?.init.body as Uint8Array)).toEqual([0xd2, 1, 2, 3]);
  });

  it("asks for SSE the same way, and compares the type without parameters or case", async () => {
    const { send, calls } = open(() =>
      stream({
        "content-type": "Text/Event-Stream; charset=utf-8",
        "cratestack-frames": "unsigned",
      }),
    );
    await send(SSE);
    expect(new Headers(calls[0]?.init.headers).get("accept")).toBe(`${SSE}, ${MEDIA}`);
  });

  it("refuses a stream that lost its marker, and cancels its body", async () => {
    const { send } = open(() => stream({ "content-type": SEQ }));
    await expect(send()).rejects.toBeInstanceOf(UnsignedFramesRefused);
    await expect(send()).rejects.toMatchObject({ code: "rejected" });
  });

  it("refuses a marker that is not exactly `unsigned`, or that is repeated", async () => {
    for (const marker of ["signed", "Unsigned", "unsigned, unsigned", ""]) {
      const { send } = open(() => stream({ "content-type": SEQ, "cratestack-frames": marker }));
      await expect(send()).rejects.toBeInstanceOf(UnsignedFramesRefused);
    }
    const doubled = new Headers({ "content-type": SEQ });
    doubled.append("cratestack-frames", "unsigned");
    doubled.append("cratestack-frames", "unsigned");
    const { send } = open(() => new Response("x", { headers: doubled }));
    await expect(send()).rejects.toBeInstanceOf(UnsignedFramesRefused);
  });

  it("refuses a forged plain 200 and a stream of another type than was asked for", async () => {
    const forged = open(() => stream({ "content-type": "application/cbor" }));
    await expect(forged.send()).rejects.toBeInstanceOf(UnsignedFramesRefused);
    const wrong = open(() => stream({ "content-type": SSE, "cratestack-frames": "unsigned" }));
    await expect(wrong.send(SEQ)).rejects.toBeInstanceOf(UnsignedFramesRefused);
    const other = open(() => stream({ "content-type": SEQ, "cratestack-frames": "unsigned" }));
    await expect(other.send(SSE)).rejects.toBeInstanceOf(UnsignedFramesRefused);
  });

  it("opens a sealed refusal and returns it, and rejects a sealed success", async () => {
    const seen = newSeen();
    const refused = open(
      () =>
        new Response(new Uint8Array([0xd2, 9]), {
          status: 403,
          headers: { "content-type": MEDIA },
        }),
      fakeCose(seen),
    );
    const response = await refused.send();
    expect(response.status).toBe(403);
    expect(response.headers.get("content-type")).toBe("application/json");
    expect(seen.opened).toBe(1);
    const success = open(
      () =>
        new Response(new Uint8Array([0xd2, 9]), {
          status: 200,
          headers: { "content-type": MEDIA },
        }),
    );
    await expect(success.send()).rejects.toMatchObject({
      code: "rejected",
      message: "a subscription answered with a sealed success",
    });
  });

  it("returns an unsigned refusal as a bodiless, tagged status, never as the server's words", async () => {
    for (const status of [401, 426, 500]) {
      const { send } = open(
        () => new Response('{"code":"permission_denied"}', { status, statusText: "No" }),
      );
      const refusal = await send();
      expect(refusal.status).toBe(status);
      expect(refusal.body).toBeNull();
      expect((refusal as unknown as Record<symbol, unknown>)[UNSIGNED_REFUSAL]).toBe(true);
    }
  });

  it("does not tag an opened (sealed) refusal", async () => {
    const { send } = open(
      () => new Response(new Uint8Array([0xd2, 9]), { status: 403, headers: { "content-type": MEDIA } }),
    );
    const refusal = await send();
    expect((refusal as unknown as Record<symbol, unknown>)[UNSIGNED_REFUSAL]).toBeUndefined();
  });

  it("sends the open with redirect: error, and refuses an answer that was redirected anyway", async () => {
    const { send, calls } = open(() => stream({ "content-type": SEQ, "cratestack-frames": "unsigned" }));
    await send();
    expect(calls[0]?.init.redirect).toBe("error");
    const redirected = open(() => {
      const response = stream({ "content-type": SEQ, "cratestack-frames": "unsigned" });
      Object.defineProperty(response, "redirected", { value: true });
      return response;
    });
    await expect(redirected.send()).rejects.toMatchObject({ code: "rejected" });
  });

  it("seals every open afresh: two opens, two different sealed bodies", async () => {
    const seen = newSeen();
    let counter = 0;
    const cose: CoseClient = {
      ...fakeCose(seen),
      async sealRequest(payload, binding) {
        seen.sealed.push({ payload, binding });
        counter += 1;
        return new Uint8Array([0xd2, counter]);
      },
    };
    const { send, calls } = open(
      () => stream({ "content-type": SEQ, "cratestack-frames": "unsigned" }),
      cose,
    );
    await send();
    await send();
    expect(seen.sealed).toHaveLength(2);
    const [first, second] = calls.map((call) => Array.from(call.init.body as Uint8Array));
    expect(first).not.toEqual(second);
  });

  it("is RPC only: a REST route is a misuse before any fetch", () => {
    const calls: unknown[] = [];
    const underlying = (async (...args: unknown[]) => calls.push(args)) as unknown as typeof fetch;
    const sealing = createSealedSubscriptions({
      cose: fakeCose(newSeen()),
      contracts: { "GET /$procs/hinted": SHA },
    });
    for (const ssePath of ["/$procs/hinted", "/v1/$procs/hinted", "/rpc/subscribe/other.op"]) {
      let thrown: unknown;
      try {
        sealing.fetchFor({ opId: "procedure.hinted", ssePath }, underlying);
      } catch (error) {
        thrown = error;
      }
      expect(thrown).toBeInstanceOf(CoseFailure);
      expect((thrown as CoseFailure).code).toBe("misuse");
    }
    expect(calls).toHaveLength(0);
  });

  it("never opens in plain: an op with no contract or no client is a SealRequired", () => {
    const underlying = (async () => new Response()) as unknown as typeof fetch;
    const missing = createSealedSubscriptions({ cose: fakeCose(newSeen()), contracts: {} });
    expect(() => missing.fetchFor(TARGET, underlying)).toThrow(SealRequired);
    const declined = createSealedSubscriptions({
      cose: () => undefined,
      contracts: { [TARGET.opId]: SHA },
    });
    expect(() => declined.fetchFor(TARGET, underlying)).toThrow(SealRequired);
  });

  it("refuses a call that is not a POST asking for a stream", async () => {
    const { send } = open(() => stream({ "content-type": SEQ, "cratestack-frames": "unsigned" }));
    await expect(send("application/json")).rejects.toMatchObject({ code: "misuse" });
  });
});

describe("createSealedFetch never seals a stream", () => {
  it("refuses a call whose Accept names a stream type, before any fetch", async () => {
    const calls: unknown[] = [];
    const fetchFn = createSealedFetch({
      cose: fakeCose(newSeen()),
      contracts: { "GET /$procs/hinted": SHA, "GET /widgets": SHA },
      fetch: (async (...args: unknown[]) => calls.push(args)) as unknown as typeof fetch,
    });
    for (const accept of [SSE, SEQ, `${SEQ}, application/cbor`, "Text/Event-Stream; q=1"]) {
      await expect(
        fetchFn("https://x/$procs/hinted", { headers: { accept } }),
      ).rejects.toMatchObject({
        code: "misuse",
      });
    }
    await expect(fetchFn("https://x/widgets", { headers: { accept: SEQ } })).rejects.toBeInstanceOf(
      CoseFailure,
    );
    expect(calls).toHaveLength(0);
  });
});
