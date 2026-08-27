// Composable interceptor chains for `CratestackRpcRuntime` (issue #182,
// extended by issue #277).
//
// Each `RpcLink` wraps the next link in the chain and terminates in the
// real network call. Passing no `links` reduces to exactly today's
// behavior — the chain built in `CratestackRpcRuntime`'s constructor
// collapses to the terminal call unchanged when the array is empty, so
// this is a true no-op, not just a documented convention. `RpcLink`
// only ever sees unary `call()` and `batch()` traffic.
//
// A link's `next` re-runs everything below it in the chain — the real
// fetch and any links declared after it — never just the terminal fetch.
// A retry link that should retry only the network attempt belongs last
// (closest to the terminal call); one that should also re-trigger e.g.
// an auth-refresh link is composed by declaring that link earlier in
// the array.
//
// `stream()` runs through a SEPARATE chain, `RpcStreamLink` /
// `streamLinks` below — deliberately not a variant of `RpcLink`. A
// `Response`-shaped link contract can't work for streaming (a link
// wanting to inspect/retry would need to clone a streamed body,
// defeating the point of streaming), so streaming links are
// async-generator-shaped instead: they consume the `AsyncIterable` of
// `RpcStreamFrame`s `next()` hands them and yield their own frames
// onward. See `docs/design/streaming-rpc-links.md` §4-5 for the full
// design rationale (why two chains, not one; why not just reuse
// `RpcLinkRequest`).

import type { CratestackRpcCodec, RpcErrorBody } from "./runtime.js";

/** One request going through the chain — either a single unary call
 *  (`kind: "unary"`) or an already-assembled `/rpc/batch` call
 *  (`kind: "batch"`, `input` is the `RpcRequest[]` array). `input` is
 *  raw, not-yet-codec-encoded — only the terminal call encodes it, so a
 *  link can inspect or rewrite it before it's serialized. */
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

/** A link's return value. Wraps the raw `Response`. A link may
 *  `.clone()` it to inspect the body but must never consume the
 *  original response itself — only the runtime's own response reader
 *  may, so the body isn't double-consumed. */
export interface RpcLinkResponse {
  readonly response: Response;
}

export type RpcLinkNext = (request: RpcLinkRequest) => Promise<RpcLinkResponse>;

/** Wraps `next` — call it to continue the chain (running the terminal
 *  fetch and any links declared after this one), or skip it to
 *  short-circuit entirely (e.g. a batching link that queues the call
 *  instead of forwarding it downstream). */
export type RpcLink = (request: RpcLinkRequest, next: RpcLinkNext) => Promise<RpcLinkResponse>;

/** Reference link (issue #182 acceptance criteria): logs each call's
 *  kind, op id, outcome, and duration. Never touches `response.body`. */
export function createLoggerLink(logger: Pick<Console, "info" | "error"> = console): RpcLink {
  return async (request, next) => {
    const start = Date.now();
    logger.info(`[rpc] -> ${request.kind} ${request.opId}`);
    try {
      const result = await next(request);
      logger.info(`[rpc] <- ${request.opId} ${result.response.status} (${Date.now() - start}ms)`);
      return result;
    } catch (error) {
      logger.error(`[rpc] x ${request.opId} failed (${Date.now() - start}ms)`, error);
      throw error;
    }
  };
}

/** One `stream()` call going through the stream chain. Deliberately NOT
 *  a variant of `RpcLinkRequest` — `RpcLinkRequest.kind` is
 *  `"unary" | "batch"`, and existing/future `RpcLink`s may switch on it
 *  exhaustively; adding a `"stream"` value there would be a breaking
 *  change for any such link. A stream also has no `urls.batch()` to
 *  speak of, so reusing the whole shape would carry a dead field too.
 *  `input` is raw, not-yet-codec-encoded, same as `RpcLinkRequest`. */
export interface RpcStreamLinkRequest {
  readonly opId: string;
  readonly input: unknown;
  readonly headers: Headers;
  readonly signal: AbortSignal | null;
  readonly codec: CratestackRpcCodec;
  readonly fetchFn: typeof fetch;
  readonly url: string;
}

/** One item out of a stream: either a decoded output value, or — for a
 *  genuinely-incremental `application/cbor-seq` response that failed
 *  partway through (issue #281) — the mid-stream error sentinel. Kept
 *  as a discriminated union rather than a thrown exception so the
 *  *chain itself* stays exception-free, consistent with how the
 *  existing unary/batch contract already works (`RpcLink` deals in
 *  `{ response: Response }` values, including error responses — only
 *  code outside the chain decides to turn that into a thrown error). A
 *  link author who wants to observe a mid-stream failure just checks
 *  `frame.kind`. */
export type RpcStreamFrame<O = unknown> =
  | { readonly kind: "output"; readonly output: O }
  | { readonly kind: "error"; readonly error: RpcErrorBody };

export type RpcStreamLinkNext = (request: RpcStreamLinkRequest) => AsyncIterable<RpcStreamFrame>;

/** A streaming link wraps `next` — call it to continue the chain
 *  (running the terminal fetch + boundary scan and any links declared
 *  after this one) and consume its `AsyncIterable`, yielding its own
 *  frames onward. No body-cloning problem: there's no single `Response`
 *  object in this contract to clone in the first place. */
export type RpcStreamLink = (
  request: RpcStreamLinkRequest,
  next: RpcStreamLinkNext,
) => AsyncIterable<RpcStreamFrame>;

/** Reference stream link, mirroring `createLoggerLink()`: logs
 *  start/frame-count/duration on completion, and on both channels a
 *  stream can fail through — a `{ kind: "error" }` frame (the in-band
 *  mid-stream sentinel) and a thrown exception (a transport failure,
 *  e.g. a malformed or truncated body). Proves a real link can
 *  consume-and-re-yield a stream chain without breaking streaming. */
export function createLoggerStreamLink(logger: Pick<Console, "info" | "error"> = console): RpcStreamLink {
  return async function* (request, next) {
    const start = Date.now();
    logger.info(`[rpc] -> stream ${request.opId}`);
    let count = 0;
    try {
      for await (const frame of next(request)) {
        if (frame.kind === "error") {
          logger.error(
            `[rpc] x stream ${request.opId} failed after ${count} frame(s) (${Date.now() - start}ms)`,
            frame.error,
          );
        } else {
          count++;
        }
        yield frame;
      }
      logger.info(`[rpc] <- stream ${request.opId} (${count} frame(s), ${Date.now() - start}ms)`);
    } catch (error) {
      logger.error(
        `[rpc] x stream ${request.opId} threw after ${count} frame(s) (${Date.now() - start}ms)`,
        error,
      );
      throw error;
    }
  };
}