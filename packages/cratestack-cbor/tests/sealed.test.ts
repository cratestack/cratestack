// The fetch-boundary sealing (issue #1010): header plumbing around a
// `CoseClient`. A fake client here, so these run in every CI lane; the same
// plumbing against the real Rust client and the shared vectors is
// `sealed-vectors.test.ts`.
import {
  type CoseCallBinding,
  type CoseClient,
  CoseFailure,
  type CoseOpened,
} from "@cratestack/ts-types";
import { describe, expect, it } from "vitest";
import { createSealedFetch, createSealedLink } from "../src/sealed.js";

const MEDIA = 'application/cose; cose-type="cose-sign1"';
const SHA = "ab".repeat(32);

interface Seen {
  sealed: { payload: Uint8Array; binding: CoseCallBinding }[];
  opened: { status: number; payloadType: string | undefined }[];
}

function fakeCose(seen: Seen, reply: Partial<CoseOpened> = {}): CoseClient {
  return {
    mode: "sign1",
    mediaType: MEDIA,
    kid: new Uint8Array(8),
    canonicalQuery: (raw) => raw.split("&").sort().join("&"),
    contractHeader: () => "CONTRACT-SEL",
    selectorHeaders: (binding) =>
      binding.payloadType === undefined || binding.payloadType === "application/cbor"
        ? {}
        : { "Cratestack-Payload-Type": binding.payloadType },
    async sealRequest(payload, binding) {
      seen.sealed.push({ payload, binding });
      return new Uint8Array([0xd2, 1, 2, 3]);
    },
    async openResponse(_body, _binding, _sealed, status, payloadType) {
      seen.opened.push({ status, payloadType });
      return {
        payload: new Uint8Array([0xa0]),
        payloadType: "application/cbor",
        kid: new Uint8Array(8),
        alg: "ed25519",
        thumbprint: new Uint8Array(32),
        ...reply,
      };
    },
  };
}

const coseResponse = (status = 200, headers: Record<string, string> = {}) =>
  new Response(status === 204 ? null : new Uint8Array([0xd2, 9]), {
    status,
    headers: { "content-type": MEDIA, ...headers },
  });

const newSeen = (): Seen => ({ sealed: [], opened: [] });

describe("createSealedLink (RPC)", () => {
  const request = (opId: string, kind: "unary" | "batch" = "unary") => ({
    kind,
    opId,
    fetchFn: undefined as unknown as typeof fetch,
  });

  it("seals a unary call as its op id, binding what the request carries", async () => {
    const seen = newSeen();
    let sent: { url: string; init: RequestInit } | undefined;
    const link = createSealedLink({ cose: fakeCose(seen), contracts: { "procedure.pay": SHA } });
    const out = await link(
      {
        ...request("procedure.pay"),
        fetchFn: async (u, i) => {
          sent = { url: String(u), init: i ?? {} };
          return coseResponse();
        },
      },
      async (next) =>
        next.fetchFn("https://x/rpc/procedure.pay", {
          method: "POST",
          headers: {
            "content-type": "application/vnd.cratestack.compact+cbor",
            accept: "application/vnd.cratestack.compact+cbor, application/cbor",
            "idempotency-key": "k-1",
            "cratestack-contract": "STALE",
          },
          body: new Uint8Array([1, 2]),
        }),
    );
    const { binding, payload } = seen.sealed[0] as Seen["sealed"][number];
    expect(Array.from(payload)).toEqual([1, 2]);
    expect(binding).toMatchObject({
      method: "POST",
      route: "procedure.pay",
      pathParams: [],
      idempotencyKey: "k-1",
      payloadType: "application/vnd.cratestack.compact+cbor",
      payloadAccept: ["application/vnd.cratestack.compact+cbor", "application/cbor"],
    });
    expect(binding.query).toBeUndefined();
    const headers = new Headers(sent?.init.headers);
    expect(headers.get("content-type")).toBe(MEDIA);
    expect(headers.get("accept")).toBe(MEDIA);
    expect(headers.get("cratestack-contract")).toBe("CONTRACT-SEL");
    expect(headers.get("cratestack-payload-type")).toBe("application/vnd.cratestack.compact+cbor");
    expect(headers.get("idempotency-key")).toBe("k-1");
    expect(Array.from(sent?.init.body as Uint8Array)).toEqual([0xd2, 1, 2, 3]);
    expect(out.headers.get("content-type")).toBe("application/cbor");
    expect(Array.from(new Uint8Array(await out.arrayBuffer()))).toEqual([0xa0]);
  });

  it("sends an op without a contract, or one the source declines, unsealed", async () => {
    const seen = newSeen();
    let calls = 0;
    const plain = (async () => {
      calls++;
      return new Response("{}");
    }) as unknown as typeof fetch;
    for (const link of [
      createSealedLink({ cose: fakeCose(seen), contracts: {} }),
      createSealedLink({ cose: undefined, contracts: { "procedure.pay": SHA } }),
      createSealedLink({ cose: () => undefined, contracts: { "procedure.pay": SHA } }),
    ]) {
      await link({ ...request("procedure.pay"), fetchFn: plain }, (next) =>
        next.fetchFn("https://x", { method: "POST" }),
      );
    }
    expect(calls).toBe(3);
    expect(seen.sealed).toHaveLength(0);
  });

  it("picks the client per call: Sign1 before a session, Mac0 after", async () => {
    const [before, after] = [newSeen(), newSeen()];
    let session = false;
    const link = createSealedLink({
      cose: ({ route }) =>
        route === "procedure.login" ? fakeCose(before) : session ? fakeCose(after) : undefined,
      contracts: { "procedure.login": SHA, "procedure.pay": SHA },
    });
    const run = (op: string) =>
      link({ ...request(op), fetchFn: async () => coseResponse() }, (next) =>
        next.fetchFn("https://x", { method: "POST" }),
      );
    await run("procedure.login");
    session = true;
    await run("procedure.pay");
    expect([before.sealed.length, after.sealed.length]).toEqual([1, 1]);
  });

  it("seals /rpc/batch as CBOR both ways, and refuses JSON", async () => {
    const seen = newSeen();
    const link = createSealedLink({ cose: fakeCose(seen), contracts: { batch: SHA } });
    const run = (type: string) =>
      link({ ...request("", "batch"), fetchFn: async () => coseResponse() }, (next) =>
        next.fetchFn("https://x/rpc/batch", {
          method: "POST",
          headers: { "content-type": type },
          body: "[]",
        }),
      );
    await run("application/cbor");
    expect(seen.sealed[0]?.binding.route).toBe("batch");
    await expect(run("application/json")).rejects.toMatchObject({ code: "misuse" });
  });
});

describe("createSealedFetch (REST)", () => {
  const sealing = (cose: CoseClient, seen: Seen, reply: () => Response = () => coseResponse()) => {
    const calls: { url: string; init: RequestInit }[] = [];
    const fetchFn = createSealedFetch({
      cose,
      basePath: "/api",
      contracts: {
        "GET /widgets/{id}": SHA,
        "GET /widgets/search": SHA,
        "PUT /accounts/{account}/payments/{id}": SHA,
        "GET /health-nope": SHA,
      },
      fetch: (async (u: RequestInfo | URL, i?: RequestInit) => {
        calls.push({ url: String(u), init: i ?? {} });
        return reply();
      }) as typeof fetch,
    });
    return { fetchFn, calls, seen };
  };

  it("matches the template and path params, canonicalises the query in Rust, binds If-Match", async () => {
    const seen = newSeen();
    const { fetchFn } = sealing(fakeCose(seen), seen);
    await fetchFn("https://x/api/accounts/acc%2042/payments/p7?b=2&a=1", {
      method: "PUT",
      headers: { "content-type": "application/json", "if-match": '"3"' },
      body: '{"n":1}',
    });
    expect(seen.sealed[0]?.binding).toMatchObject({
      method: "PUT",
      route: "/accounts/{account}/payments/{id}",
      pathParams: ["acc 42", "p7"],
      query: "a=1&b=2",
      ifMatch: '"3"',
      payloadType: "application/json",
    });
    expect(new TextDecoder().decode(seen.sealed[0]?.payload)).toBe('{"n":1}');
  });

  it("prefers the more literal template, seals a GET with an empty payload, passes unmatched calls through", async () => {
    const seen = newSeen();
    const { fetchFn, calls } = sealing(fakeCose(seen), seen);
    await fetchFn("https://x/api/widgets/search");
    expect(seen.sealed[0]?.binding.route).toBe("/widgets/search");
    expect(seen.sealed[0]?.payload).toHaveLength(0);
    await fetchFn("https://x/api/elsewhere", { method: "GET", headers: { a: "b" } });
    expect(seen.sealed).toHaveLength(1);
    expect(new Headers(calls[1]?.init.headers).get("a")).toBe("b");
  });

  it("maps the reply: payload type header, no-body statuses, content type", async () => {
    const seen = newSeen();
    const { fetchFn } = sealing(
      fakeCose(seen, { payloadType: "application/json", payload: new TextEncoder().encode("{}") }),
      seen,
      () =>
        coseResponse(200, { "cratestack-payload-type": "application/json", "content-length": "2" }),
    );
    const response = await fetchFn("https://x/api/widgets/w1");
    expect(seen.opened[0]).toEqual({ status: 200, payloadType: "application/json" });
    expect(response.headers.get("content-type")).toBe("application/json");
    expect(response.headers.has("cratestack-payload-type")).toBe(false);
    expect(await response.text()).toBe("{}");

    const empty = sealing(fakeCose(newSeen(), { payload: new Uint8Array() }), newSeen(), () =>
      coseResponse(204),
    );
    expect((await empty.fetchFn("https://x/api/widgets/w1")).status).toBe(204);
  });

  it("returns an unsigned refusal untouched, and never believes an unsigned success", async () => {
    const seen = newSeen();
    for (const status of [401, 415, 426]) {
      const { fetchFn } = sealing(
        fakeCose(seen),
        seen,
        () =>
          new Response('{"code":"x"}', { status, headers: { "content-type": "application/json" } }),
      );
      expect((await fetchFn("https://x/api/widgets/w1")).status).toBe(status);
    }
    const { fetchFn } = sealing(
      fakeCose(seen),
      seen,
      () => new Response('{"id":"forged"}', { headers: { "content-type": "application/json" } }),
    );
    await expect(fetchFn("https://x/api/widgets/w1")).rejects.toMatchObject({
      code: "rejected",
      message: "",
    });
    expect(seen.opened).toHaveLength(0);
  });

  it("rejects when the reply does not open, with the client's own failure", async () => {
    const seen = newSeen();
    const cose = fakeCose(seen);
    cose.openResponse = async () => {
      throw new CoseFailure("rejected", "");
    };
    const { fetchFn } = sealing(cose, seen);
    await expect(fetchFn("https://x/api/widgets/w1")).rejects.toBeInstanceOf(CoseFailure);
  });
});
