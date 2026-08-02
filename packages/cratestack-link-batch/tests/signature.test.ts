import type { RpcLinkRequest } from "@cratestack/ts-types";
import { jsonCodec } from "@cratestack/ts-types/test-harness";
import { describe, expect, it } from "vitest";
import { batchSignature, effectiveConfig } from "../src/signature.js";
import type { BatchLinkOptions } from "../src/types.js";

function fakeRequest(overrides: Partial<RpcLinkRequest> = {}): RpcLinkRequest {
  return {
    kind: "unary",
    opId: "procedure.echo",
    input: null,
    headers: new Headers(),
    signal: null,
    codec: jsonCodec,
    fetchFn: fetch,
    urls: {
      unary: (id) => `https://example.test/rpc/${id}`,
      batch: () => "https://example.test/rpc/batch",
    },
    ...overrides,
  };
}

describe("batchSignature", () => {
  it("is identical for header sets that differ only in insertion order", () => {
    const a = effectiveConfig(
      fakeRequest({
        headers: new Headers([
          ["x-a", "1"],
          ["x-b", "2"],
        ]),
      }),
      {},
    );
    const b = effectiveConfig(
      fakeRequest({
        headers: new Headers([
          ["x-b", "2"],
          ["x-a", "1"],
        ]),
      }),
      {},
    );

    expect(batchSignature(a)).toBe(batchSignature(b));
  });

  it("is identical for header sets that differ only in casing (Headers normalizes case)", () => {
    const a = effectiveConfig(fakeRequest({ headers: new Headers({ "X-Tenant": "acme" }) }), {});
    const b = effectiveConfig(fakeRequest({ headers: new Headers({ "x-tenant": "acme" }) }), {});

    expect(batchSignature(a)).toBe(batchSignature(b));
  });

  it("ignores Idempotency-Key entirely — it's frame-level, not part of the request envelope", () => {
    const a = effectiveConfig(
      fakeRequest({ headers: new Headers({ "idempotency-key": "order-a" }) }),
      {},
    );
    const b = effectiveConfig(
      fakeRequest({ headers: new Headers({ "idempotency-key": "order-b" }) }),
      {},
    );

    expect(batchSignature(a)).toBe(batchSignature(b));
  });

  it("differs when any other header value differs", () => {
    const a = effectiveConfig(
      fakeRequest({ headers: new Headers({ authorization: "Bearer a" }) }),
      {},
    );
    const b = effectiveConfig(
      fakeRequest({ headers: new Headers({ authorization: "Bearer b" }) }),
      {},
    );

    expect(batchSignature(a)).not.toBe(batchSignature(b));
  });

  it("differs for distinct fetchFn references even with identical headers", () => {
    const fetchA = (async () => new Response()) as typeof fetch;
    const fetchB = (async () => new Response()) as typeof fetch;

    const a = effectiveConfig(fakeRequest({ fetchFn: fetchA }), {});
    const b = effectiveConfig(fakeRequest({ fetchFn: fetchB }), {});

    expect(batchSignature(a)).not.toBe(batchSignature(b));
  });

  it("differs for distinct resolved batch URLs", () => {
    const a = effectiveConfig(
      fakeRequest({ urls: { unary: (id) => id, batch: () => "https://a.test/rpc/batch" } }),
      {},
    );
    const b = effectiveConfig(
      fakeRequest({ urls: { unary: (id) => id, batch: () => "https://b.test/rpc/batch" } }),
      {},
    );

    expect(batchSignature(a)).not.toBe(batchSignature(b));
  });
});

describe("effectiveConfig", () => {
  it("merges link-level header overrides on top of the request's own", () => {
    const options: BatchLinkOptions = { headers: { "x-link-default": "1" } };
    const config = effectiveConfig(
      fakeRequest({ headers: new Headers({ authorization: "Bearer a" }) }),
      options,
    );

    expect(config.headers.get("authorization")).toBe("Bearer a");
    expect(config.headers.get("x-link-default")).toBe("1");
  });

  it("link-level headers override a same-named header on the request", () => {
    const options: BatchLinkOptions = { headers: { "x-tenant": "link-wins" } };
    const config = effectiveConfig(
      fakeRequest({ headers: new Headers({ "x-tenant": "request" }) }),
      options,
    );

    expect(config.headers.get("x-tenant")).toBe("link-wins");
  });
});
