// Pinned local copies of the wire/link contract generated into every
// CrateStack `transport rpc` project by
// `crates/cratestack-client-typescript/templates/src/rpc-links.ts.j2` and
// `rpc-runtime.ts.j2` (issue #182, extended by issue #277). Kept as
// plain interfaces/function types deliberately — a generated project's
// `CratestackRpcRuntime` is a per-project class with no shared import
// path, so this package can't (and doesn't need to) import it;
// TypeScript's structural typing means any object shaped like these
// types is assignable into a generated client's `links`/`streamLinks`
// array, regardless of which project generated it.
//
// `./cbor-seq` (re-exported below) is the one piece of this package
// that ISN'T just types — the boundary-scanner is real, non-trivial
// logic (issue #277's own highest-risk piece), pinned here as an actual
// implementation, not merely a type, the same way the generated
// `rpc-cbor-seq.ts.j2`/`rpc-cbor-item.ts.j2` templates are.

/** One request going through the chain — mirrors the generated
 *  `RpcLinkRequest`. */
export interface RpcLinkRequest {
  readonly kind: "unary" | "batch";
  readonly opId: string;
  readonly input: unknown;
  readonly headers: Headers;
  readonly signal: AbortSignal | null;
  readonly idempotencyKey?: string;
  readonly codec: CratestackRpcCodec;
  readonly fetchFn: typeof fetch;
  readonly urls: {
    unary(opId: string): string;
    batch(): string;
  };
}

/** Mirrors the generated `RpcLinkResponse`. */
export interface RpcLinkResponse {
  readonly response: Response;
}

export type RpcLinkNext = (request: RpcLinkRequest) => Promise<RpcLinkResponse>;

/** Mirrors the generated `RpcLink`. */
export type RpcLink = (request: RpcLinkRequest, next: RpcLinkNext) => Promise<RpcLinkResponse>;

/** One `stream()` call going through the stream chain — mirrors the
 *  generated `RpcStreamLinkRequest` (issue #277). Deliberately NOT a
 *  variant of `RpcLinkRequest`; see the generated `rpc-links.ts.j2`
 *  template's doc comment on `RpcStreamLinkRequest` for why. */
export interface RpcStreamLinkRequest {
  readonly opId: string;
  readonly input: unknown;
  readonly headers: Headers;
  readonly signal: AbortSignal | null;
  readonly codec: CratestackRpcCodec;
  readonly fetchFn: typeof fetch;
  readonly url: string;
}

/** Mirrors the generated `RpcStreamFrame`: one item out of a stream,
 *  either a decoded output value or — for a genuinely-incremental
 *  `application/cbor-seq` response that failed partway through (issue
 *  #281) — the mid-stream error sentinel. */
export type RpcStreamFrame<O = unknown> =
  | { readonly kind: "output"; readonly output: O }
  | { readonly kind: "error"; readonly error: RpcErrorBody };

/** Mirrors the generated `RpcStreamLinkNext`. */
export type RpcStreamLinkNext = (request: RpcStreamLinkRequest) => AsyncIterable<RpcStreamFrame>;

/** Mirrors the generated `RpcStreamLink`. */
export type RpcStreamLink = (
  request: RpcStreamLinkRequest,
  next: RpcStreamLinkNext,
) => AsyncIterable<RpcStreamFrame>;

/** Mirrors the generated `CratestackRpcCodec`. */
export interface CratestackRpcCodec {
  readonly contentType: string;
  encode(value: unknown): BodyInit;
  decode(bytes: Uint8Array): unknown;
}

/** Wire shape of a single `/rpc/batch` request frame — mirrors the
 *  generated `RpcRequest`. */
export interface RpcRequest<I = unknown> {
  id: number;
  op: string;
  input: I;
  idem?: string;
}

/** Wire shape of a single `/rpc/batch` response frame — mirrors the
 *  generated `RpcResponseFrame`. */
export interface RpcResponseFrame<O = unknown> {
  id: number;
  output?: O;
  error?: RpcErrorBody;
}

/** Mirrors the generated `RpcErrorBody`. */
export interface RpcErrorBody {
  code: string;
  message: string;
  details?: unknown;
}

/** Structural match for `CratestackRpcRuntime.call()` (see
 *  `crates/cratestack-client-typescript/templates/src/rpc-runtime.ts.j2`)
 *  — a generated client exposes this as its public `.runtime` field, so
 *  e.g. `rpcQueryOptions(client.runtime, opId, input)`
 *  (`@cratestack/adapter-tanstack-query`) or
 *  `createRpcBaseQuery(client.runtime)` (`@cratestack/adapter-rtk`) work
 *  against any generated project without either package importing its
 *  (per-project, unshared) class. Shared here rather than duplicated
 *  per adapter package, since both need the exact same contract. */
export interface RpcCaller {
  call<I, O>(opId: string, input: I, options?: { signal?: AbortSignal }): Promise<O>;
}

// `./cbor-item` is deliberately internal (the low-level single-item
// walk) — mirroring the generated package's own `src/cbor-item.ts`,
// which `src/index.ts.j2` never re-exports either. `./cbor-seq` is the
// public surface for boundary-scanning.
export * from "./cbor-seq.js";
