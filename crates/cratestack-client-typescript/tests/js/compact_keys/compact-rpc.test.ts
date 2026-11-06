// `compactKeys` on the generated RPC runtime, in the default and `--swr`
// layouts (they render the same template, and this keeps them agreeing).
// `@cratestack/cbor` is mocked with a fake `createCompactKeys`; the real
// codec still encodes and decodes the bodies.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createCborCodec } from "@cratestack/cbor";
import * as plain from "./src/runtime.js";
import * as swr from "./src/swr/runtime.js";
import type { FakeKeys } from "./fake-keys.js";

const fake = vi.hoisted(() => ({
  calls: [] as string[],
  tables: [] as Uint8Array[],
  selectorFor: (op: string): string | undefined => `sel-${op}`,
})) as FakeKeys;

vi.mock("@cratestack/cbor", async (importOriginal) => {
  const real = await importOriginal<Record<string, any>>();
  const { makeFakeCreate } = await import("./fake-keys.js");
  return { ...real, createCompactKeys: makeFakeCreate(fake, real.createCborCodec) };
});

const COMPACT_CBOR = "application/vnd.cratestack.compact+cbor";
const COMPACT_JSON = "application/vnd.cratestack.compact+json";
const COMPACT_SEQ = "application/vnd.cratestack.compact+cbor-seq";

interface Sent {
  url: string;
  headers: Headers;
  body: Uint8Array | string | null;
}

type Responder = (sent: Sent, index: number) => Response | Promise<Response>;

function transport(respond: Responder): { fetchFn: typeof fetch; sent: Sent[] } {
  const sent: Sent[] = [];
  const fetchFn: typeof fetch = async (input, init) => {
    const body = init?.body;
    const record: Sent = {
      url: String(input),
      headers: new Headers(init?.headers),
      body: typeof body === "string" ? body : body instanceof Uint8Array ? body : null,
    };
    sent.push(record);
    return respond(record, sent.length - 1);
  };
  return { fetchFn, sent };
}

async function compactCbor(value: unknown, type = COMPACT_CBOR): Promise<Response> {
  const codec = await createCborCodec();
  // fake compact form = a CBOR byte string holding the named bytes
  const named = codec.encode(value) as Uint8Array;
  return new Response(codec.encode(named) as Uint8Array, { headers: { "Content-Type": type } });
}

async function plainCbor(value: unknown, status = 200): Promise<Response> {
  const codec = await createCborCodec();
  return new Response(codec.encode(value) as Uint8Array, {
    status,
    headers: { "Content-Type": "application/cbor" },
  });
}

async function namedFromCompactBody(body: Sent["body"]): Promise<unknown> {
  const codec = await createCborCodec();
  return codec.decode(codec.decode(body as Uint8Array) as Uint8Array);
}

const layouts = [
  ["default layout", plain],
  ["--swr layout", swr],
] as const;

beforeEach(() => {
  fake.calls.length = 0;
  fake.tables.length = 0;
  fake.selectorFor = (op) => `sel-${op}`;
});

describe.each(layouts)("%s", (_name, mod) => {
  it("compactKeys off: unchanged headers, and the binding is never touched", async () => {
    const { fetchFn, sent } = transport(() => plainCbor("hi"));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", { fetch: fetchFn });
    expect(await runtime.call("procedure.echoName", { name: "a" })).toBe("hi");
    await runtime.call("procedure.echoName", { name: "b" });
    for (const request of sent) {
      expect(request.headers.get("accept")).toBe("application/cbor");
      expect(request.headers.get("content-type")).toBe("application/cbor");
      expect(request.headers.has("cratestack-contract")).toBe(false);
    }
    expect(fake.tables).toHaveLength(0);
    expect(fake.calls).toEqual([]);
  });

  it("compactKeys on: negotiates, transcodes the response, then sends compact", async () => {
    const { fetchFn, sent } = transport((_request, index) =>
      index < 2 ? compactCbor("hi") : plainCbor("late"),
    );
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });

    expect(await runtime.call("procedure.echoName", { name: "a" })).toBe("hi");
    expect(sent[0]?.headers.get("accept")).toBe(`${COMPACT_CBOR}, application/cbor`);
    expect(sent[0]?.headers.get("cratestack-contract")).toBe("sel-procedure.echoName");
    expect(sent[0]?.headers.get("content-type")).toBe("application/cbor");
    expect(await createCborCodec().then((c) => c.decode(sent[0]?.body as Uint8Array))).toEqual({
      name: "a",
    });

    expect(await runtime.call("procedure.echoName", { name: "b" })).toBe("hi");
    expect(sent[1]?.headers.get("content-type")).toBe(COMPACT_CBOR);
    expect(await namedFromCompactBody(sent[1]?.body ?? null)).toEqual({ name: "b" });
    expect(fake.calls).toContain("toCompact:procedure.echoName:request");
    expect(fake.calls).toContain("fromCompact:procedure.echoName:response");

    // a different op has not answered compact yet: its body stays named
    await runtime.call("model.Widget.create", { id: 1, name: "w" }).catch(() => undefined);
    expect(sent[2]?.headers.get("content-type")).toBe("application/cbor");

    // the tables are the generated KEY_TABLES, handed over once
    expect(fake.tables).toHaveLength(1);
    expect(Buffer.from(fake.tables[0]!).toString("base64")).toBe(mod.KEY_TABLES);
    expect(mod.KEY_TABLES.length).toBeGreaterThan(0);
  });

  it("a server that answers plain keeps this op's request body named", async () => {
    const { fetchFn, sent } = transport(() => plainCbor("hi"));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.call("procedure.echoName", { name: "a" });
    await runtime.call("procedure.echoName", { name: "b" });
    expect(sent[1]?.headers.get("accept")).toBe(`${COMPACT_CBOR}, application/cbor`);
    expect(sent[1]?.headers.get("content-type")).toBe("application/cbor");
    expect(fake.calls).toEqual([]);
  });

  it("an error body is never transcoded", async () => {
    const { fetchFn } = transport(() =>
      plainCbor({ code: "not_found", message: "nope" }, 404),
    );
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await expect(runtime.call("procedure.echoName", { name: "a" })).rejects.toMatchObject({
      code: "not_found",
    });
    expect(fake.calls).toEqual([]);
  });

  it("an op the tables do not know stays named", async () => {
    fake.selectorFor = () => undefined;
    const { fetchFn, sent } = transport(() => plainCbor("hi"));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.call("procedure.echoName", { name: "a" });
    expect(sent[0]?.headers.get("accept")).toBe("application/cbor");
    expect(sent[0]?.headers.has("cratestack-contract")).toBe(false);
  });

  it("a retry under one Idempotency-Key reuses its first encoding", async () => {
    const { fetchFn, sent } = transport(() => compactCbor("hi"));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.call("procedure.echoName", { name: "a" }, { idempotencyKey: "k1" });
    await runtime.call("procedure.echoName", { name: "a" }, { idempotencyKey: "k1" });
    await runtime.call("procedure.echoName", { name: "a" }, { idempotencyKey: "k2" });
    expect(sent.map((request) => request.headers.get("content-type"))).toEqual([
      "application/cbor",
      "application/cbor",
      COMPACT_CBOR,
    ]);
  });

  it("pins a key supplied as a header and rejects conflicting sources", async () => {
    const { fetchFn, sent } = transport(() => compactCbor("hi"));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", { fetch: fetchFn, compactKeys: true });
    await runtime.call("procedure.echoName", { name: "a" }, { headers: { "idempotency-key": "k" } });
    await runtime.call("procedure.echoName", { name: "a" }, { headers: { "Idempotency-Key": "k" } });
    expect(sent.map((request) => request.headers.get("content-type"))).toEqual(["application/cbor", "application/cbor"]);
    await expect(runtime.call("procedure.echoName", {}, { idempotencyKey: "a", headers: { "idempotency-key": "b" } })).rejects.toThrow();
    expect(sent).toHaveLength(2);
  });

  it("retains an old keyed encoding after more than 512 different keys", async () => {
    const { fetchFn, sent } = transport(() => compactCbor("hi"));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn, compactKeys: true,
    });
    for (let key = 0; key < 513; key += 1) {
      await runtime.call("procedure.echoName", { name: "a" }, { idempotencyKey: `key-${key}` });
    }
    await runtime.call("procedure.echoName", { name: "a" }, { idempotencyKey: "key-0" });
    expect(sent[513]?.headers.get("content-type")).toBe(sent[0]?.headers.get("content-type"));
    expect(sent[513]?.body).toEqual(sent[0]?.body);
  });

  it.each(["cbor", "json"] as const)("persists %s compact bytes across runtime recreation", async (format) => {
    const { fetchFn, sent } = transport(() => format === "json"
      ? Promise.resolve(new Response(JSON.stringify({ c: JSON.stringify("hi") }), { headers: { "Content-Type": COMPACT_JSON } }))
      : compactCbor("hi"));
    const opts = { fetch: fetchFn, compactKeys: true, ...(format === "json" ? { codec: mod.jsonRpcCodec } : {}) };
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", opts);
    await runtime.call("procedure.echoName", { name: "warm" });
    const prepared = await runtime.prepare("procedure.echoName", { name: "a" }, { idempotencyKey: "persist" });
    expect(sent).toHaveLength(1);
    const restored = JSON.parse(JSON.stringify(prepared));
    const restarted = new mod.CratestackRpcRuntime("http://x.invalid", opts);
    await restarted.executePrepared(restored);
    await restarted.executePrepared(restored);
    expect(sent[1]?.headers.get("content-type")).toBe(format === "json" ? COMPACT_JSON : COMPACT_CBOR);
    expect(sent[2]?.body).toEqual(sent[1]?.body);
    await expect(restarted.executePrepared({ ...restored, contract: "changed" })).rejects.toThrow();
  });

  it("the JSON codec negotiates compact+json", async () => {
    const { fetchFn, sent } = transport((_request, index) =>
      Promise.resolve(
        new Response(JSON.stringify({ c: JSON.stringify("hi") }), {
          headers: { "Content-Type": index < 2 ? COMPACT_JSON : "application/json" },
        }),
      ),
    );
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      codec: mod.jsonRpcCodec,
      compactKeys: true,
    });
    expect(await runtime.call("procedure.echoName", { name: "a" })).toBe("hi");
    expect(sent[0]?.headers.get("accept")).toBe(`${COMPACT_JSON}, application/json`);
    await runtime.call("procedure.echoName", { name: "b" });
    expect(sent[1]?.headers.get("content-type")).toBe(COMPACT_JSON);
    expect(JSON.parse(sent[1]?.body as string)).toEqual({ c: JSON.stringify({ name: "b" }) });
  });

  it("a cbor-seq stream transcodes each item and leaves the error item alone", async () => {
    const codec = await createCborCodec();
    const bytes = (value: unknown) => codec.encode(value) as Uint8Array;
    const item = (value: unknown) => bytes(bytes(value));
    // tag 48900 (0xd9 0xbf 0x04) + a named error map: the in-band error item
    const errorItem = Uint8Array.from([0xd9, 0xbf, 0x04, ...bytes({ code: "internal", message: "boom" })]);
    const seq = (items: Uint8Array[]) =>
      new Response(Uint8Array.from(items.flatMap((entry) => [...entry])), {
        headers: { "Content-Type": COMPACT_SEQ },
      });
    const { fetchFn, sent } = transport((_request, index) =>
      Promise.resolve(index === 0 ? seq([item(1), item(2), errorItem]) : seq([item(3)])),
    );
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });

    const seen: unknown[] = [];
    await expect(
      (async () => {
        for await (const value of runtime.stream("procedure.ticks", { count: 2 })) {
          seen.push(value);
        }
      })(),
    ).rejects.toMatchObject({ code: "internal" });
    expect(seen).toEqual([1, 2]);
    expect(sent[0]?.headers.get("accept")).toBe(
      `${COMPACT_SEQ}, application/cbor-seq, application/cbor`,
    );
    expect(sent[0]?.headers.get("cratestack-contract")).toBe("sel-procedure.ticks");
    expect(sent[0]?.headers.get("content-type")).toBe("application/cbor");

    for await (const value of runtime.stream("procedure.ticks", { count: 1 })) {
      expect(value).toBe(3);
    }
    expect(sent[1]?.headers.get("content-type")).toBe(COMPACT_CBOR);
    expect(await namedFromCompactBody(sent[1]?.body ?? null)).toEqual({ count: 1 });
  });

  it("a batch is never compact", async () => {
    const { fetchFn, sent } = transport(() => plainCbor([]));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.batch([{ id: 0, op: "procedure.echoName", input: { name: "a" } }]);
    expect(sent[0]?.headers.get("accept")).toBe("application/cbor");
    expect(sent[0]?.headers.has("cratestack-contract")).toBe(false);
    expect(fake.calls).toEqual([]);
  });
  it("rejects altered bindings and credentials before sending", async () => {
    const { fetchFn, sent } = transport(() => plainJson({}));
    const runtime = new mod.CratestackRpcRuntime("http://x.invalid", { fetch: fetchFn, codec: mod.jsonRpcCodec,
      headers: { Authorization: "Bearer secret", Cookie: "session=secret", "X-API-Key": "secret" } });
    const saved = await runtime.prepare("procedure.echoName", { name: "a" }, { idempotencyKey: "k" });
    expect(JSON.stringify(saved)).not.toContain("secret");
    const elsewhere = new mod.CratestackRpcRuntime("http://other.invalid", { fetch: fetchFn, codec: mod.jsonRpcCodec });
    await expect(elsewhere.executePrepared(saved)).rejects.toThrow();
    await expect(runtime.executePrepared({ ...saved, headers: [...saved.headers, ["authorization", "Bearer injected"]] })).rejects.toThrow();
    await expect(runtime.prepare("procedure.echoName", {}, { idempotencyKey: "a", headers: { "IDEMPOTENCY-KEY": "b" } })).rejects.toThrow();
    expect(sent).toHaveLength(0);
  });

});
