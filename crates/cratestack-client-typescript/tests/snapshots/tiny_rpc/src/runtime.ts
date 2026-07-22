// Generated CrateStack TypeScript RPC runtime for `transport rpc` schemas.
//
// Speaks the `/rpc/{op_id}` and `/rpc/batch` URL space defined by
// `cratestack-axum::rpc`. Unary calls POST the codec-encoded input
// directly; sequence/streaming calls POST the input and read back an
// `application/cbor-seq`-shaped body.

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

/** Plugs into {@link CratestackRpcRuntime} to control how request bodies
 *  are encoded and response bodies are decoded. `contentType` is sent as
 *  both the request `Content-Type` and the response `Accept` header, so
 *  it must match a `Content-Type` the server's `CodecSet` actually
 *  serves — e.g. `"application/cbor"` for a backend whose `CodecSet`
 *  defaults to CBOR in production. The runtime ships {@link jsonRpcCodec}
 *  by default; pass a different one via `CratestackRpcClientOptions.codec`. */
export interface CratestackRpcCodec {
  readonly contentType: string;
  encode(value: unknown): BodyInit;
  decode(bytes: Uint8Array): unknown;
}

/** Default codec — the runtime's behavior before `codec` existed. */
export const jsonRpcCodec: CratestackRpcCodec = {
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

export interface CratestackRpcClientOptions {
  basePath?: string;
  fetch?: typeof fetch;
  headers?: HeadersInit | (() => HeadersInit | Promise<HeadersInit>);
  /** Codec for request/response bodies. Defaults to {@link jsonRpcCodec}. */
  codec?: CratestackRpcCodec;
}

export interface CratestackRpcCallOptions {
  headers?: HeadersInit;
  signal?: AbortSignal;
  /** Per-call idempotency key — propagated to the server as the
   *  `Idempotency-Key` HTTP header on unary calls. */
  idempotencyKey?: string;
}

/** Wire shape of a single batch request frame. Mirrors the server-side
 *  `cratestack_core::rpc::RpcRequest`. */
export interface RpcRequest<I = JsonValue> {
  id: number;
  op: string;
  input: I;
  idem?: string;
}

/** Wire shape of a single batch response frame. Mirrors the server-side
 *  `cratestack_core::rpc::RpcResponseFrame`. */
export interface RpcResponseFrame<O = JsonValue> {
  id: number;
  output?: O;
  error?: RpcErrorBody;
}

/** Wire shape of an RPC error body. Mirrors the server-side
 *  `cratestack_core::rpc::RpcErrorBody`. */
export interface RpcErrorBody {
  code: RpcErrorCode | string;
  message: string;
  details?: unknown;
}

/** Stable gRPC-style error codes the server emits. Open string union
 *  so a future server-side code lands without breaking compilation. */
export type RpcErrorCode =
  | "invalid_argument"
  | "unauthenticated"
  | "permission_denied"
  | "not_found"
  | "conflict"
  | "failed_precondition"
  | "internal";

/** Thrown by `CratestackRpcRuntime` when a remote call fails. Carries
 *  the wire-shaped `RpcErrorBody` directly so callers can switch on
 *  `error.code` (`"not_found"`, `"unauthenticated"`, etc.). */
export class CratestackRpcError extends Error {
  readonly status: number;
  readonly code: RpcErrorCode | string;
  readonly details: unknown;
  readonly body: RpcErrorBody;

  constructor(status: number, body: RpcErrorBody) {
    super(`RPC call failed with code ${body.code} (status ${status}): ${body.message}`);
    this.name = "CratestackRpcError";
    this.status = status;
    this.code = body.code;
    this.details = body.details;
    this.body = body;
  }
}

/** Transport-level error (network failure, malformed response,
 *  unsupported content-type). Distinct from {@link CratestackRpcError}
 *  which always means the server itself emitted a `RpcErrorBody`. */
export class CratestackRpcTransportError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CratestackRpcTransportError";
  }
}

const CBOR_SEQ_CONTENT_TYPE = "application/cbor-seq";

export class CratestackRpcRuntime {
  readonly origin: string;
  readonly basePath: string;
  readonly fetchFn: typeof fetch;
  readonly codec: CratestackRpcCodec;
  readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;

  constructor(origin: string, options: CratestackRpcClientOptions = {}) {
    this.origin = origin.replace(/\/+$/, "");
    this.basePath = options.basePath ?? "/api";
    this.fetchFn = options.fetch ?? fetch;
    this.codec = options.codec ?? jsonRpcCodec;
    this.defaultHeaders = options.headers;
  }

  /** POST /rpc/{op_id} — unary call. */
  async call<I, O>(opId: string, input: I, options: CratestackRpcCallOptions = {}): Promise<O> {
    const headers = await this.buildHeaders(options.headers);
    headers.set("Accept", this.codec.contentType);
    headers.set("Content-Type", this.codec.contentType);
    if (options.idempotencyKey !== undefined) {
      headers.set("Idempotency-Key", options.idempotencyKey);
    }

    const response = await this.fetchFn(this.url(`/rpc/${encodeURIComponent(opId)}`), {
      method: "POST",
      headers,
      body: this.codec.encode(input ?? null),
      signal: options.signal ?? null,
    });

    return (await this.readUnaryResponse(response)) as O;
  }

  /** POST /rpc/batch — batched calls. Per-frame errors do not poison
   *  the batch; each `RpcResponseFrame` reports its own success or
   *  failure. */
  async batch<O = JsonValue>(
    requests: RpcRequest[],
    options: CratestackRpcCallOptions = {},
  ): Promise<RpcResponseFrame<O>[]> {
    const headers = await this.buildHeaders(options.headers);
    headers.set("Accept", this.codec.contentType);
    headers.set("Content-Type", this.codec.contentType);

    const response = await this.fetchFn(this.url("/rpc/batch"), {
      method: "POST",
      headers,
      body: this.codec.encode(requests),
      signal: options.signal ?? null,
    });

    return (await this.readUnaryResponse(response)) as RpcResponseFrame<O>[];
  }

  /** POST /rpc/{op_id} — sequence-returning call. Yields one `O` per
   *  frame the server emits. Decoded from a single body matching
   *  `codec.contentType` when the server picks that codec; CBOR-seq
   *  streaming responses aren't supported by any codec yet (see below). */
  async *stream<O>(
    opId: string,
    input: unknown,
    options: CratestackRpcCallOptions = {},
  ): AsyncIterable<O> {
    const headers = await this.buildHeaders(options.headers);
    headers.set("Accept", `${CBOR_SEQ_CONTENT_TYPE}, ${this.codec.contentType}`);
    headers.set("Content-Type", this.codec.contentType);

    const response = await this.fetchFn(this.url(`/rpc/${encodeURIComponent(opId)}`), {
      method: "POST",
      headers,
      body: this.codec.encode(input ?? null),
      signal: options.signal ?? null,
    });

    if (!response.ok) {
      const body = await readErrorBody(response, this.codec);
      throw new CratestackRpcError(response.status, body);
    }

    const contentType = response.headers.get("Content-Type") ?? "";
    if (matchesContentType(contentType, this.codec.contentType)) {
      // Server picked the configured codec — body is a single array of `O`.
      const bytes = new Uint8Array(await response.arrayBuffer());
      if (bytes.length === 0) {
        return;
      }
      const items = this.codec.decode(bytes) as O[];
      for (const item of items) {
        yield item;
      }
      return;
    }

    // CBOR sequence — caller is expected to provide a decoder. The
    // default runtime ships JSON only; CBOR streaming decoding is a
    // TODO so we surface the unsupported case rather than corrupting
    // the iterator.
    // TODO: wire a CBOR-seq decoder (e.g. `cbor-x` streaming reader)
    //       so streaming works against CBOR servers.
    throw new CratestackRpcTransportError(
      `streaming over ${CBOR_SEQ_CONTENT_TYPE} is not yet supported by the default runtime`,
    );
  }

  private async readUnaryResponse(response: Response): Promise<unknown> {
    if (response.ok) {
      if (response.status === 204) {
        return undefined;
      }
      const bytes = new Uint8Array(await response.arrayBuffer());
      return this.codec.decode(bytes);
    }

    throw new CratestackRpcError(response.status, await readErrorBody(response, this.codec));
  }

  private async buildHeaders(extra?: HeadersInit): Promise<Headers> {
    const headers = new Headers(await resolveHeaders(this.defaultHeaders));
    for (const [key, value] of new Headers(extra)) {
      headers.set(key, value);
    }
    return headers;
  }

  private url(path: string): string {
    const normalizedBase = this.basePath === "/" ? "" : this.basePath.replace(/\/+$/, "");
    const normalizedPath = path.startsWith("/") ? path : `/${path}`;
    return new URL(`${normalizedBase}${normalizedPath}`, `${this.origin}/`).toString();
  }
}

// Compares against the media type only (ignores `; charset=...` etc.) so
// e.g. a `codec.contentType` of `"application/cbor"` doesn't accidentally
// match an `"application/cbor-seq"` response — `includes()` would.
function matchesContentType(header: string, expected: string): boolean {
  const mediaType = header.split(";", 1)[0]?.trim() ?? "";
  return mediaType === expected;
}

async function resolveHeaders(
  headers: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined,
): Promise<HeadersInit | undefined> {
  if (typeof headers === "function") {
    return headers();
  }
  return headers;
}

async function readErrorBody(response: Response, codec: CratestackRpcCodec): Promise<RpcErrorBody> {
  const bytes = new Uint8Array(await response.arrayBuffer().catch(() => new ArrayBuffer(0)));
  if (bytes.length === 0) {
    return { code: "internal", message: `RPC call returned status ${response.status}` };
  }
  try {
    const parsed = codec.decode(bytes) as RpcErrorBody;
    if (typeof parsed === "object" && parsed !== null && typeof parsed.code === "string") {
      return parsed;
    }
    return {
      code: "internal",
      message: `RPC call returned status ${response.status} with an unrecognized error body`,
    };
  } catch {
    return {
      code: "internal",
      message: `RPC call returned status ${response.status} with an undecodable error body`,
    };
  }
}