// `compactKeys` on the generated REST runtime (JSON only), in the default and
// `--swr` layouts. `@cratestack/cbor` is mocked with a fake
// `createCompactKeys`, loaded by the runtime's dynamic `import()`.
import { beforeEach, describe, expect, it, vi } from "vitest";
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

const COMPACT_JSON = "application/vnd.cratestack.compact+json";

interface Sent {
  url: string;
  method: string;
  headers: Headers;
  body: string | null;
}

function transport(respond: (sent: Sent, index: number) => Response): {
  fetchFn: typeof fetch;
  sent: Sent[];
} {
  const sent: Sent[] = [];
  const fetchFn: typeof fetch = async (input, init) => {
    const record: Sent = {
      url: String(input),
      method: String(init?.method),
      headers: new Headers(init?.headers),
      body: typeof init?.body === "string" ? init.body : null,
    };
    sent.push(record);
    return respond(record, sent.length - 1);
  };
  return { fetchFn, sent };
}

// fake compact form = {"c": "<named json text>"}
const compactResponse = (value: unknown) =>
  new Response(JSON.stringify({ c: JSON.stringify(value) }), {
    headers: { "Content-Type": COMPACT_JSON },
  });
const plainResponse = (value: unknown, status = 200) =>
  new Response(JSON.stringify(value), { status, headers: { "Content-Type": "application/json" } });

beforeEach(() => {
  fake.calls.length = 0;
  fake.tables.length = 0;
  fake.selectorFor = (op) => `sel-${op}`;
});

describe.each([
  ["default layout", plain],
  ["--swr layout", swr],
] as const)("%s", (_name, mod) => {
  it("a 415 to a compact request sends the op back to named, and a retry under its key too", async () => {
    const { fetchFn, sent } = transport((_request, index) =>
      index === 0
        ? compactResponse({ id: 1 })
        : index === 1
          ? plainResponse({ code: "unsupported_media_type" }, 415)
          : plainResponse({ id: 1 }),
    );
    const runtime = new mod.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    const options = { headers: { "Idempotency-Key": "k" } };
    await runtime.post("/widgets", { name: "warm" });
    await expect(runtime.post("/widgets", { name: "a" }, options)).rejects.toBeDefined();
    expect(sent[1]?.headers.get("content-type")).toBe(COMPACT_JSON);
    await runtime.post("/widgets", { name: "a" }, options);
    await runtime.post("/widgets", { name: "b" });
    expect(sent[2]?.headers.get("content-type")).toBe("application/json");
    expect(sent[3]?.headers.get("content-type")).toBe("application/json");
  });

  it("compactKeys off: unchanged headers, and the binding is never touched", async () => {
    const { fetchFn, sent } = transport(() => plainResponse({ id: 1 }));
    const runtime = new mod.CratestackRuntime("http://x.invalid", { fetch: fetchFn });
    await runtime.get("/widgets/1");
    await runtime.post("/widgets", { name: "a" });
    expect(sent[0]?.headers.get("accept")).toBe("application/json");
    expect(sent[1]?.headers.get("content-type")).toBe("application/json");
    for (const request of sent) {
      expect(request.headers.has("cratestack-contract")).toBe(false);
    }
    expect(fake.tables).toHaveLength(0);
    expect(fake.calls).toEqual([]);
  });

  it("retains the first encoding when Idempotency-Key is supplied in headers", async () => {
    const { fetchFn, sent } = transport(() => compactResponse({ id: 1 }));
    const runtime = new mod.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn, compactKeys: true,
    });
    const options = { headers: { "iDeMpOtEnCy-KeY": "key-0" } };
    await runtime.post("/widgets", { name: "a" }, options);
    await runtime.post("/widgets", { name: "a" }, options);
    expect(sent[1]?.headers.get("content-type")).toBe(sent[0]?.headers.get("content-type"));
    expect(sent[1]?.body).toEqual(sent[0]?.body);
  });

  it("persists compact bytes before the first attempt across runtime recreation", async () => {
    const { fetchFn, sent } = transport(() => compactResponse({ id: 1 }));
    const runtime = new mod.CratestackRuntime("http://x.invalid", { fetch: fetchFn, compactKeys: true });
    await runtime.post("/widgets", { name: "warm" });
    const prepared = await runtime.prepare("POST", "/widgets", { body: { name: "a" }, query: { a: 1 }, headers: { "Idempotency-Key": "persist" } });
    expect(sent).toHaveLength(1);
    const restored = JSON.parse(JSON.stringify(prepared));
    const restarted = new mod.CratestackRuntime("http://x.invalid", { fetch: fetchFn, compactKeys: true });
    await restarted.executePrepared(restored);
    await restarted.executePrepared(restored);
    expect(sent[1]?.headers.get("content-type")).toBe(COMPACT_JSON);
    expect(sent[2]?.body).toEqual(sent[1]?.body);
    // A replay carries the schema digest exactly as an ordinary request does.
    expect(sent[1]?.headers.get("x-cratestack-schema-sha")).toBe(sent[0]?.headers.get("x-cratestack-schema-sha"));
    await expect(restarted.executePrepared({ ...restored, contract: "changed" })).rejects.toThrow();
  });

  it("compactKeys on: keys the op by method and route template, then sends compact", async () => {
    const { fetchFn, sent } = transport((_request, index) =>
      index === 0 ? plainResponse({ id: 1 }) : compactResponse({ id: 1, name: "a" }),
    );
    const runtime = new mod.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });

    // GET /widgets/{id}: the concrete path resolves to its template
    await runtime.get("/widgets/42");
    expect(sent[0]?.headers.get("accept")).toBe(`${COMPACT_JSON}, application/json`);
    expect(sent[0]?.headers.get("cratestack-contract")).toBe("sel-GET /widgets/{id}");

    // POST /widgets answers compact: transcoded back, flag set
    expect(await runtime.post("/widgets", { name: "a" })).toEqual({ id: 1, name: "a" });
    expect(sent[1]?.headers.get("cratestack-contract")).toBe("sel-POST /widgets");
    expect(sent[1]?.headers.get("content-type")).toBe("application/json");
    expect(sent[1]?.body).toBe(JSON.stringify({ name: "a" }));

    // the next POST /widgets goes out compact
    await runtime.post("/widgets", { name: "b" });
    expect(sent[2]?.headers.get("content-type")).toBe(COMPACT_JSON);
    expect(JSON.parse(sent[2]?.body ?? "null")).toEqual({ c: JSON.stringify({ name: "b" }) });

    // the other op did not answer compact: still named
    await runtime.get("/widgets/7");
    expect(fake.calls).toContain("toCompactJson:POST /widgets:request");
    expect(fake.calls).toContain("fromCompactJson:POST /widgets:response");
    expect(fake.tables).toHaveLength(1);
    expect(Buffer.from(fake.tables[0]!).toString("base64")).toBe(mod.KEY_TABLES);
  });

  it("an error body is never transcoded", async () => {
    const { fetchFn } = transport(() => plainResponse({ code: "NOT_FOUND", message: "no" }, 404));
    const runtime = new mod.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await expect(runtime.get("/widgets/1")).rejects.toMatchObject({ status: 404 });
    expect(fake.calls).toEqual([]);
  });

  it("a route the tables do not know stays named", async () => {
    const { fetchFn, sent } = transport(() => plainResponse({}));
    const runtime = new mod.CratestackRuntime("http://x.invalid", {
      fetch: fetchFn,
      compactKeys: true,
    });
    await runtime.get("/nowhere/1");
    expect(sent[0]?.headers.get("accept")).toBe("application/json");
    expect(sent[0]?.headers.has("cratestack-contract")).toBe(false);
  });
  it("rejects injected credentials and duplicate headers before sending", async () => {
    const { fetchFn, sent } = transport(() => plainResponse({}));
    const runtime = new mod.CratestackRuntime("http://x.invalid", { fetch: fetchFn,
      headers: { Authorization: "Bearer secret", Cookie: "session=secret", "X-API-Key": "secret" } });
    const saved = await runtime.prepare("POST", "/widgets", { body: { name: "a" }, headers: { "Idempotency-Key": "k" } });
    expect(JSON.stringify(saved)).not.toContain("secret");
    await expect(runtime.executePrepared({ ...saved, headers: [...saved.headers, ["authorization", "Bearer injected"]] })).rejects.toThrow();
    await expect(runtime.executePrepared({ ...saved, headers: [...saved.headers, ["IDEMPOTENCY-KEY", "other"]] })).rejects.toThrow();
    expect(sent).toHaveLength(0);
  });

  it("preserves absent and present conditional headers across fresh defaults", async () => {
    const { fetchFn, sent } = transport(() => plainResponse({}));
    let defaults: Record<string, string> = {};
    const runtime = new mod.CratestackRuntime("http://x.invalid", { fetch: fetchFn, headers: () => defaults });
    const saved = await runtime.prepare("POST", "/widgets", { body: {}, headers: { "idempotency-key": "original" } });
    defaults = { "idempotency-key": "replacement", "if-match": "new-etag", authorization: "fresh" };
    await runtime.executePrepared(saved);
    expect(sent[0]?.headers.get("idempotency-key")).toBe("original");
    expect(sent[0]?.headers.get("if-match")).toBeNull();
    expect(sent[0]?.headers.get("authorization")).toBe("fresh");
    const bad = { ...saved, headers: saved.headers.filter(([name]) => name !== "idempotency-key") };
    await expect(runtime.executePrepared(bad)).rejects.toThrow();
    expect(sent).toHaveLength(1);
  });

  it("rejects URL userinfo before preparation and execution", async () => {
    const { fetchFn, sent } = transport(() => plainResponse({}));
    const unsafe = new mod.CratestackRuntime("http://user:secret@x.invalid", { fetch: fetchFn });
    await expect(unsafe.prepare("POST", "/widgets", { body: {}, headers: { "Idempotency-Key": "k" } })).rejects.toThrow();
    const safe = new mod.CratestackRuntime("http://x.invalid", { fetch: fetchFn });
    const saved = await safe.prepare("POST", "/widgets", { body: {}, headers: { "Idempotency-Key": "k" } });
    await expect(safe.executePrepared({ ...saved, url: saved.url.replace("//", "//user:secret@") })).rejects.toThrow();
    expect(sent).toHaveLength(0);
  });

});
