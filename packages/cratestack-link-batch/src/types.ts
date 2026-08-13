import type { CratestackRpcCodec, RpcLinkRequest, RpcLinkResponse } from "@cratestack/ts-types";

export interface BatchLinkOptions {
  /** Scheduling window. Omitted (default) uses `queueMicrotask` — calls
   *  fired synchronously in the same tick (e.g. inside `Promise.all`)
   *  collapse into one `/rpc/batch` request. Pass a millisecond value to
   *  widen the window across ticks (e.g. debounce bursts from unrelated
   *  components). */
  windowMs?: number;
  /** Maximum requests per flushed `/rpc/batch` call. Applied **per
   *  partition** (see {@link batchSignature}), not across the whole
   *  flush — a partition larger than this splits into several
   *  concurrent requests. Default: unbounded; the server enforces no
   *  size limit either (see `docs/design/rpc-transport.md` §3.2), but a
   *  huge fan-in may still be worth capping client-side. */
  maxBatchSize?: number;
  /** Returns a dedupe key for a queued call, or `null` to never
   *  collapse it with anything else. Two calls in the same partition
   *  that return the same non-null key become ONE `/rpc/batch` frame,
   *  and every caller resolves from that single result.
   *
   *  Default: only collapse calls that share an explicit
   *  `idempotencyKey` — that's already the caller's own signal that the
   *  call is safe to treat as a repeat. Calls with no idempotency key
   *  are never auto-collapsed, since the server does no dedup of its
   *  own and silently merging two textually-identical but unmarked
   *  mutations would be unsafe. Pass a custom `dedupe` for full
   *  value-based collapsing, or `() => null` to disable dedup entirely
   *  and only batch. */
  dedupe?: (request: RpcLinkRequest) => string | null;
  /** Headers merged **over** the shared headers of each partition when
   *  synthesizing its `/rpc/batch` request. Use this to declare the
   *  aggregate request's own headers rather than inheriting whatever
   *  the queued calls happened to carry. */
  headers?: HeadersInit;
  /** `fetch` used for synthesized `/rpc/batch` requests, overriding the
   *  queued calls' own. */
  fetchFn?: typeof fetch;
  /** Codec used to encode the `/rpc/batch` body and decode its response
   *  frames, overriding the queued calls' own. Note each caller's
   *  individual result is still re-encoded with *its own* codec, so a
   *  generated runtime always decodes what it expects. */
  codec?: CratestackRpcCodec;
}

/** One queued call awaiting a flush. */
export interface QueueEntry {
  readonly request: RpcLinkRequest;
  readonly resolve: (value: RpcLinkResponse) => void;
  readonly reject: (reason: unknown) => void;
}

/** Calls collapsed into a single `/rpc/batch` frame by `dedupe`. */
export interface Group {
  readonly key: string | null;
  readonly entries: QueueEntry[];
}

/** The transport config a synthesized `/rpc/batch` request is issued
 *  with, after applying any {@link BatchLinkOptions} overrides. Every
 *  entry within a partition resolves to an equal config by
 *  construction — that is what the partition *is*. */
export interface EffectiveConfig {
  readonly headers: Headers;
  readonly fetchFn: typeof fetch;
  readonly codec: CratestackRpcCodec;
  readonly batchUrl: string;
}
