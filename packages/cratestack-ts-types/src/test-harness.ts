import type { RpcLink, RpcLinkNext, RpcLinkRequest } from "./index.js";

export const jsonCodec = {
  contentType: "application/json",
  encode(value: unknown): BodyInit {
    return JSON.stringify(value ?? null);
  },
  decode(bytes: Uint8Array): unknown {
    if (bytes.length === 0) {
      return undefined;
    }
    return JSON.parse(new TextDecoder().decode(bytes));
  },
};

const terminalLink: RpcLinkNext = async (request) => {
  const url = request.kind === "batch" ? request.urls.batch() : request.urls.unary(request.opId);
  const response = await request.fetchFn(url, {
    method: "POST",
    headers: request.headers,
    body: request.codec.encode(request.input),
    signal: request.signal,
  });
  return { response };
};

/** Mirrors `CratestackRpcRuntime`'s chain construction and `call()`/
 *  `batch()` request-building exactly (see
 *  `crates/cratestack-client-typescript/templates/src/rpc-runtime.ts.j2`),
 *  so these tests exercise the real generated contract rather than a
 *  reimplementation of it. Exported (as `@cratestack/ts-types/test-harness`,
 *  not part of the public `.` entry point) so every other `@cratestack/*`
 *  package's own test suite can exercise its `RpcLink` against a real
 *  chain instead of each maintaining its own copy. */
export class FakeRuntime {
  private readonly chain: RpcLinkNext;
  private readonly fetchFn: typeof fetch;

  constructor(fetchFn: typeof fetch, links: RpcLink[] = []) {
    this.fetchFn = fetchFn;
    this.chain = links.reduceRight<RpcLinkNext>(
      (next, link) => (request) => link(request, next),
      terminalLink,
    );
  }

  async call<O>(
    opId: string,
    input: unknown,
    opts: { signal?: AbortSignal; idempotencyKey?: string } = {},
  ): Promise<O> {
    const request: RpcLinkRequest = {
      kind: "unary",
      opId,
      input: input ?? null,
      headers: new Headers(),
      signal: opts.signal ?? null,
      ...(opts.idempotencyKey !== undefined ? { idempotencyKey: opts.idempotencyKey } : {}),
      codec: jsonCodec,
      fetchFn: this.fetchFn,
      urls: {
        unary: (id: string) => `https://example.test/rpc/${id}`,
        batch: () => "https://example.test/rpc/batch",
      },
    };
    const { response } = await this.chain(request);
    if (response.status === 204) {
      return undefined as O;
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (!response.ok) {
      const body = jsonCodec.decode(bytes) as { code: string; message: string };
      throw new Error(`rpc error ${body.code}: ${body.message}`);
    }
    return jsonCodec.decode(bytes) as O;
  }

  async batch(requests: unknown[]): Promise<unknown> {
    const request: RpcLinkRequest = {
      kind: "batch",
      opId: "batch",
      input: requests,
      headers: new Headers(),
      signal: null,
      codec: jsonCodec,
      fetchFn: this.fetchFn,
      urls: {
        unary: (id: string) => `https://example.test/rpc/${id}`,
        batch: () => "https://example.test/rpc/batch",
      },
    };
    const { response } = await this.chain(request);
    const bytes = new Uint8Array(await response.arrayBuffer());
    return jsonCodec.decode(bytes);
  }
}
