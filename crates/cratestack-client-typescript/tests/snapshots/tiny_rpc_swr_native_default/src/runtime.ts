// Generated CrateStack TypeScript RPC runtime for `transport rpc` schemas.
//
// Speaks the `/rpc/{op_id}` and `/rpc/batch` URL space defined by
// `cratestack-axum::rpc`. Unary calls POST the codec-encoded input
// directly; sequence/streaming calls POST the input and read back an
// `application/cbor-seq`-shaped body.
//
// `call()`/`batch()` run through the composable `links` chain (see
// `./links`, issue #182) before reaching the real network call;
// `stream()` runs through its own separate `streamLinks` chain (issue
// #277) terminating in a boundary-scan of the response instead of a
// single `Response` read — see `./links` for why the two chains are
// separate contracts.

import type { RpcLink, RpcLinkNext, RpcLinkRequest, RpcStreamLink, RpcStreamLinkNext } from "./links.js";
import { terminalStreamLink } from "./stream-terminal.js";
import { encodeBinaryAsJson, encodeWireFields } from "./models.js";
import { createCborCodec } from "@cratestack/cbor";

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

// Hex-encoded SHA-256 of the schema's source bytes (issue #178) — baked in
// at generation time from `TypeScriptGeneratorConfig::schema_sha256`. Sent
// as `x-cratestack-schema-sha` on every unary/streaming/batch call so a
// client compiled against a stale `.cstack` schema shows up as a
// server-side `tracing::warn!`, never a rejection. Empty when the CLI
// wasn't given a schema fingerprint (e.g. this crate used as a library
// directly, or a test) — the header is simply omitted in that case.
export const SCHEMA_SHA256: string = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA_SHA_HEADER = "x-cratestack-schema-sha";

/** Plugs into {@link CratestackRpcRuntime} to control how request bodies
 *  are encoded and response bodies are decoded. `contentType` is sent as
 *  both the request `Content-Type` and the response `Accept` header, so
 *  it must match a `Content-Type` the server's `CodecSet` actually
 *  serves — e.g. `"application/cbor"` for a backend whose `CodecSet`
 *  defaults to CBOR in production. The runtime
 *  resolves `@cratestack/cbor`'s `createCborCodec()` by default (issue
 *  #746); pass a different one via `CratestackRpcClientOptions.codec`. */
export interface CratestackRpcCodec {
  readonly contentType: string;
  encode(value: unknown): BodyInit;
  decode(bytes: Uint8Array): unknown;
}

/** Fallback codec — pass `codec: jsonRpcCodec` explicitly to opt back
 *  into it; `@cratestack/cbor`'s native codec is the default (issue
 *  #746). */
export const jsonRpcCodec: CratestackRpcCodec = {
  contentType: "application/json",
  encode(value: unknown): BodyInit {
    // `encodeBinaryAsJson` rewrites every `Uint8Array` (a `Bytes` field's
    // client-side type) into the integer array the wire uses. Applied
    // here rather than in `terminalLink`'s shared `encodeWireFields` pass
    // on purpose: a native CBOR codec wants the real `Uint8Array` so it
    // can emit a compact byte string, and only this JSON path needs the
    // lossy-looking conversion. See that function's own doc comment for
    // why `JSON.stringify` can't do it.
    //
    // (Deliberately no package name in this comment: `native_cbor: false`
    // builds assert that `runtime.ts` never mentions the native codec
    // package at all — see `tests/native_cbor_generator.rs`.)
    return JSON.stringify(encodeBinaryAsJson(value) ?? null);
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
  /** Codec for request/response bodies. Defaults to `@cratestack/cbor`'s
   *  native codec (issue #746). */
  codec?: CratestackRpcCodec;
  /** Composable interceptor chain (issue #182) — logging, retry,
   *  auth-refresh, batching, etc. Runs in array order, each link
   *  wrapping the next, terminating in the real network call. Omitted
   *  or empty is a true no-op: requests are byte-identical to not
   *  having this option at all. Applies to `call()`/`batch()` only —
   *  `stream()` uses the separate `streamLinks` chain below. */
  links?: RpcLink[];
  /** Composable interceptor chain for `stream()` (issue #277) — same
   *  ordering contract as `links`, but frame-shaped instead of
   *  `Response`-shaped (see `./links`'s `RpcStreamLink` doc comment for
   *  why). Omitted or empty is a true no-op. */
  streamLinks?: RpcStreamLink[];
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
  | "resource_exhausted"
  | "unavailable"
  | "deadline_exceeded"
  | "canceled"
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

/** Thrown by `stream()` when the response ends in the CBOR-tagged
 *  mid-stream error sentinel (issue #281): a `@stream` procedure that
 *  failed partway through, after headers were already sent as `200`.
 *  Deliberately a separate class from {@link CratestackRpcError} rather
 *  than reusing it with a fabricated `status` — by the time a mid-stream
 *  error is observable, the real HTTP status has already been committed
 *  to the wire as `200`, so there is no honest status code to report. */
export class CratestackRpcStreamError extends Error {
  readonly code: RpcErrorCode | string;
  readonly details: unknown;
  readonly body: RpcErrorBody;

  constructor(body: RpcErrorBody) {
    super(`RPC stream failed with code ${body.code}: ${body.message}`);
    this.name = "CratestackRpcStreamError";
    this.code = body.code;
    this.details = body.details;
    this.body = body;
  }
}

/** Media type for a genuinely-incremental sequence response (a
 *  `@stream` procedure negotiated over `Accept: application/cbor-seq` —
 *  see `cratestack-axum::transport::stream_sequence`). Exported so
 *  `./stream-terminal` can share this exact constant rather than a
 *  second copy of the literal. */
export const CBOR_SEQ_CONTENT_TYPE = "application/cbor-seq";

/** The chain's terminal link — performs the real network call. Always
 *  runs last regardless of what `links` declares; `reduceRight` wraps
 *  every declared link around this.
 *
 *  `encodeWireFields(request.input)` runs immediately before
 *  `codec.encode()` — the one place every unary AND batch request body
 *  reaches a codec, so this single call site covers a `batch()` payload's
 *  own per-frame `input` fields too (the encoder recurses through the
 *  frame array unconditionally). See that function's own doc comment
 *  (`models.ts.j2`) for why this runs for every codec, not just the
 *  native one. */
const terminalLink: RpcLinkNext = async (request) => {
  const url = request.kind === "batch" ? request.urls.batch() : request.urls.unary(request.opId);
  const response = await request.fetchFn(url, {
    method: "POST",
    headers: request.headers,
    body: request.codec.encode(encodeWireFields(request.input)),
    signal: request.signal,
  });
  return { response };
};

export class CratestackRpcRuntime {
  readonly origin: string;
  readonly basePath: string;
  readonly fetchFn: typeof fetch;
  // `@cratestack/cbor`'s `createCborCodec()` is async on every platform
  // (issue #746) — Node normalizes to async purely for call-site parity
  // with the browser build, whose WASM instantiation is genuinely async
  // (see `@cratestack/cbor`'s own `src/node.ts` doc comment). The
  // constructor stays synchronous regardless: `explicitCodec` captures an
  // eagerly-supplied `options.codec` (which needs no async resolution at
  // all), and `codecPromise` lazily memoizes the native codec the first
  // time it's actually needed, so `createCborCodec()` runs at most once
  // per runtime instance rather than once per request — see
  // `resolveCodec()` below for why a rejected attempt is NOT memoized.
  private readonly explicitCodec: CratestackRpcCodec | undefined;
  private codecPromise: Promise<CratestackRpcCodec> | undefined;
  readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;
  private readonly chain: RpcLinkNext;
  private readonly streamChain: RpcStreamLinkNext;

  constructor(origin: string, options: CratestackRpcClientOptions = {}) {
    this.origin = origin.replace(/\/+$/, "");
    this.basePath = options.basePath ?? "/api";
    // `.bind(globalThis)` — see `rest-runtime.ts.j2`'s identical line
    // for why: some browsers' `fetch` throws `TypeError: Illegal
    // invocation` when invoked with a receiver other than the global
    // object, which storing the bare function on `this.fetchFn` does.
    this.fetchFn = options.fetch ?? fetch.bind(globalThis);
    this.explicitCodec = options.codec;
    this.defaultHeaders = options.headers;
    // Empty `links`/`streamLinks` collapses `reduceRight` to the
    // terminal link unchanged — byte-identical request as before either
    // option existed.
    this.chain = (options.links ?? []).reduceRight<RpcLinkNext>(
      (next, link) => (request) => link(request, next),
      terminalLink,
    );
    this.streamChain = (options.streamLinks ?? []).reduceRight<RpcStreamLinkNext>(
      (next, link) => (request) => link(request, next),
      terminalStreamLink,
    );
  }
  /** Resolves the runtime's codec, awaited right after `buildHeaders()`
   *  at every already-`async` call site (issue #746) — that ordering
   *  (headers first, codec second) is deliberate: it matches the
   *  `--no-native-cbor` path's ordering exactly, so a user's
   *  `options.headers` callback fires at the same point in both. An
   *  explicit `options.codec` always wins and needs no async resolution;
   *  otherwise `@cratestack/cbor`'s `createCborCodec()` is invoked and
   *  memoized on first use, never more than once per runtime instance.
   *
   *  Only a *successful* resolution is memoized — mirrors
   *  `@cratestack/cbor-web`'s own `ensureInitialized()`
   *  (`packages/cratestack-cbor-web/src/index.ts`), whose doc comment
   *  explains why: plain `??=` never re-evaluates for a settled
   *  *rejected* promise, so a transient failure (e.g. a missing/
   *  misconfigured WASM asset, or a napi load failure) would otherwise
   *  brick every later call on this runtime instance, replaying the same
   *  rejection forever instead of giving the next call a fresh retry. */
  private resolveCodec(): Promise<CratestackRpcCodec> {
    if (this.explicitCodec !== undefined) {
      return Promise.resolve(this.explicitCodec);
    }
    // Assigned through a local so TypeScript's narrowing of the
    // just-created (never-undefined) promise survives the return —
    // narrowing a mutable `this.` property does not survive across the
    // `this.codecPromise = ...` assignment above it, which otherwise
    // widens this method's return type to `Promise<CratestackRpcCodec> |
    // undefined`. The retry-on-rejection behavior is unchanged: the
    // `.catch()` below still clears `this.codecPromise` so the next call
    // gets a fresh `createCborCodec()` attempt.
    const pending =
      this.codecPromise ??
      (this.codecPromise = createCborCodec().catch((error: unknown) => {
        this.codecPromise = undefined;
        throw error;
      }));
    return pending;
  }

  /** POST /rpc/{op_id} — unary call. */
  async call<I, O>(opId: string, input: I, options: CratestackRpcCallOptions = {}): Promise<O> {
    const headers = await this.buildHeaders(options.headers);
    const codec = await this.resolveCodec();
    headers.set("Accept", codec.contentType);
    headers.set("Content-Type", codec.contentType);
    if (options.idempotencyKey !== undefined) {
      headers.set("Idempotency-Key", options.idempotencyKey);
    }

    const { response } = await this.chain({
      kind: "unary",
      opId,
      input: input ?? null,
      headers,
      signal: options.signal ?? null,
      ...(options.idempotencyKey !== undefined ? { idempotencyKey: options.idempotencyKey } : {}),
      codec,
      fetchFn: this.fetchFn,
      urls: this.linkUrls(),
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
    const codec = await this.resolveCodec();
    headers.set("Accept", codec.contentType);
    headers.set("Content-Type", codec.contentType);

    const { response } = await this.chain({
      kind: "batch",
      opId: "batch",
      input: requests,
      headers,
      signal: options.signal ?? null,
      codec,
      fetchFn: this.fetchFn,
      urls: this.linkUrls(),
    });

    return (await this.readUnaryResponse(response)) as RpcResponseFrame<O>[];
  }

  /** POST /rpc/{op_id} — sequence-returning call. Runs through the
   *  `streamLinks` chain (issue #277), terminating in a real fetch. When
   *  the server picks the configured codec, the body is a single array
   *  of `O` decoded and yielded in one go (byte-identical to this
   *  method's pre-#277 behavior). When the server picks
   *  `application/cbor-seq` (a genuinely-streaming `@stream` procedure),
   *  items are decoded and yielded incrementally as the boundary
   *  scanner finds each one — never after buffering the whole body. A
   *  response ending in the mid-stream error sentinel (issue #281)
   *  throws {@link CratestackRpcStreamError} instead of yielding a final
   *  `O` — the stream ends there either way. */
  async *stream<O>(
    opId: string,
    input: unknown,
    options: CratestackRpcCallOptions = {},
  ): AsyncIterable<O> {
    const headers = await this.buildHeaders(options.headers);
    const codec = await this.resolveCodec();
    headers.set("Accept", `${CBOR_SEQ_CONTENT_TYPE}, ${codec.contentType}`);
    headers.set("Content-Type", codec.contentType);

    for await (const frame of this.streamChain({
      opId,
      input: input ?? null,
      headers,
      signal: options.signal ?? null,
      codec,
      fetchFn: this.fetchFn,
      url: this.url(`/rpc/${encodeURIComponent(opId)}`),
    })) {
      if (frame.kind === "error") {
        throw new CratestackRpcStreamError(frame.error);
      }
      yield frame.output as O;
    }
  }

  private async readUnaryResponse(response: Response): Promise<unknown> {
    const codec = await this.resolveCodec();
    if (response.ok) {
      if (response.status === 204) {
        return undefined;
      }
      const bytes = new Uint8Array(await response.arrayBuffer());
      return codec.decode(bytes);
    }

    throw new CratestackRpcError(response.status, await readErrorBody(response, codec));
  }

  private async buildHeaders(extra?: HeadersInit): Promise<Headers> {
    const headers = new Headers(await resolveHeaders(this.defaultHeaders));
    if (SCHEMA_SHA256 !== "") {
      headers.set(SCHEMA_SHA_HEADER, SCHEMA_SHA256);
    }
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

  private linkUrls(): RpcLinkRequest["urls"] {
    return {
      unary: (opId: string) => this.url(`/rpc/${encodeURIComponent(opId)}`),
      batch: () => this.url("/rpc/batch"),
    };
  }
}

// Compares against the media type only (ignores `; charset=...` etc.) so
// e.g. a `codec.contentType` of `"application/cbor"` doesn't accidentally
// match an `"application/cbor-seq"` response — `includes()` would.
// Exported so `./stream-terminal` shares this exact check rather than a
// second, potentially-drifting copy of it.
export function matchesContentType(header: string, expected: string): boolean {
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

/** Best-effort decode of a non-2xx response body as `RpcErrorBody`.
 *  Exported so `./stream-terminal`'s terminal stream link reuses the
 *  exact same fallback behavior `call()`/`batch()` already have for a
 *  malformed/empty/undecodable error body, rather than a second copy. */
export async function readErrorBody(response: Response, codec: CratestackRpcCodec): Promise<RpcErrorBody> {
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