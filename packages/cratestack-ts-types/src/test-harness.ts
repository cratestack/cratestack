import { CborSeqBoundaryScanner, classifyCborSeqItem } from "./cbor-seq.js";
import type {
  CratestackRpcCodec,
  RpcErrorBody,
  RpcLink,
  RpcLinkNext,
  RpcLinkRequest,
  RpcStreamLink,
  RpcStreamLinkNext,
  RpcStreamLinkRequest,
} from "./index.js";

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
    opts: { signal?: AbortSignal; idempotencyKey?: string; headers?: HeadersInit } = {},
  ): Promise<O> {
    // Mirrors `CratestackRpcRuntime.call()`/`buildHeaders()`: per-call
    // `options.headers` are merged in, and the caller's `idempotencyKey`
    // is written into the `Idempotency-Key` header too, not just the
    // `RpcLinkRequest.idempotencyKey` field — a link that only reads one
    // of the two (e.g. deriving a batch signature from headers) needs a
    // harness that actually sets both, or its coverage of that case is
    // vacuous. Set AFTER `opts.headers` (deliberately, matching the real
    // runtime's own order) so `opts.idempotencyKey`, the explicit typed
    // option, always wins over a same-named header the caller happened
    // to also pass in `opts.headers` — not a merge, an override.
    const headers = new Headers(opts.headers);
    if (opts.idempotencyKey !== undefined) {
      headers.set("Idempotency-Key", opts.idempotencyKey);
    }
    const request: RpcLinkRequest = {
      kind: "unary",
      opId,
      input: input ?? null,
      headers,
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

const CBOR_SEQ_CONTENT_TYPE = "application/cbor-seq";

function matchesContentType(header: string, expected: string): boolean {
  return (header.split(";", 1)[0]?.trim() ?? "") === expected;
}

/** Thrown by `FakeStreamRuntime.stream()` for the mid-stream error
 *  sentinel — mirrors `CratestackRpcStreamError`
 *  (`rpc-runtime.ts.j2`) closely enough for tests to assert on
 *  `error.body`, without pulling in the whole generated-runtime class
 *  (which, like `CratestackRpcRuntime` itself, has no shared import
 *  path from a per-project generated package). */
export class FakeStreamError extends Error {
  constructor(readonly body: RpcErrorBody) {
    super(`rpc stream error ${body.code}: ${body.message}`);
  }
}

/** Mirrors `CratestackRpcRuntime`'s stream chain construction and
 *  `terminalStreamLink` exactly (see
 *  `crates/cratestack-client-typescript/templates/src/rpc-stream-terminal.ts.j2`),
 *  so `RpcStreamLink` tests exercise the real generated contract rather
 *  than a reimplementation of it — the sibling of `FakeRuntime` above,
 *  for `stream()` instead of `call()`/`batch()`. */
export class FakeStreamRuntime {
  private readonly streamChain: RpcStreamLinkNext;
  private readonly fetchFn: typeof fetch;

  private readonly codec: CratestackRpcCodec;

  // `codec` defaults to the same `jsonCodec` `FakeRuntime` uses, but is
  // overridable so a test can drive the real `application/cbor-seq`
  // terminal-link path end to end (that path picks its behavior off
  // `request.codec.contentType`, exactly like the generated runtime).
  constructor(
    fetchFn: typeof fetch,
    streamLinks: RpcStreamLink[] = [],
    codec: CratestackRpcCodec = jsonCodec,
  ) {
    this.fetchFn = fetchFn;
    this.codec = codec;
    this.streamChain = streamLinks.reduceRight<RpcStreamLinkNext>(
      (next, link) => (request) => link(request, next),
      terminalStreamLink,
    );
  }

  async *stream<O>(
    opId: string,
    input: unknown,
    opts: { signal?: AbortSignal } = {},
  ): AsyncIterable<O> {
    const request: RpcStreamLinkRequest = {
      opId,
      input: input ?? null,
      headers: new Headers(),
      signal: opts.signal ?? null,
      codec: this.codec,
      fetchFn: this.fetchFn,
      url: `https://example.test/rpc/${opId}`,
    };
    for await (const frame of this.streamChain(request)) {
      if (frame.kind === "error") {
        throw new FakeStreamError(frame.error);
      }
      yield frame.output as O;
    }
  }
}

const terminalStreamLink: RpcStreamLinkNext = async function* (request) {
  const response = await request.fetchFn(request.url, {
    method: "POST",
    headers: request.headers,
    body: request.codec.encode(request.input),
    signal: request.signal,
  });

  if (!response.ok) {
    throw new Error(`rpc stream request failed with status ${response.status}`);
  }

  const contentType = response.headers.get("Content-Type") ?? "";
  if (matchesContentType(contentType, request.codec.contentType)) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.length === 0) {
      return;
    }
    const items = request.codec.decode(bytes) as unknown[];
    for (const item of items) {
      yield { kind: "output", output: item };
    }
    return;
  }

  if (!matchesContentType(contentType, CBOR_SEQ_CONTENT_TYPE) || response.body === null) {
    throw new Error(`streaming response had unsupported Content-Type "${contentType}"`);
  }

  const scanner = new CborSeqBoundaryScanner();
  const reader = response.body.getReader();
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      for (const itemBytes of scanner.feedChunk(value)) {
        const frame = classifyCborSeqItem(itemBytes, request.codec);
        yield frame;
        if (frame.kind === "error") {
          return;
        }
      }
    }
    if (scanner.pendingLength > 0) {
      throw new Error(
        `${CBOR_SEQ_CONTENT_TYPE} response ended with ${scanner.pendingLength} bytes buffered (truncated final item)`,
      );
    }
  } finally {
    reader.releaseLock();
  }
};
