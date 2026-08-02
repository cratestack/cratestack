import type { RpcLink, RpcLinkRequest, RpcLinkResponse } from "@cratestack/ts-types";
import { dispatchPartition } from "./dispatch.js";
import { partition } from "./signature.js";
import type { BatchLinkOptions, QueueEntry } from "./types.js";

export { batchSignature, effectiveConfig } from "./signature.js";
export type { BatchLinkOptions } from "./types.js";

const defaultDedupe = (request: RpcLinkRequest): string | null =>
  request.idempotencyKey !== undefined ? `idem:${request.idempotencyKey}` : null;

/** A batshit-style (github.com/yornaath/batshit) automatic batch
 *  scheduler, shipped as an `RpcLink` (issue #182's composition
 *  mechanism) rather than a `fetch` override — so it composes with a
 *  logger, retry, or auth-refresh link instead of clobbering them.
 *
 *  Terminal in practice: for `kind: "unary"` calls it never invokes
 *  `next` — it queues the call and later performs its own
 *  `POST /rpc/batch` requests. An explicit `runtime.batch()` call
 *  (`kind: "batch"`) passes straight through via `next`, since it's
 *  already the shape this link would otherwise build.
 *
 *  Each flush is split into partitions by transport config — headers,
 *  `fetch`, codec, batch URL (see `batchSignature`) — and every
 *  partition is sent as its own request, so no call's headers are ever
 *  dropped in favor of another's (issue #273). Calls sharing a config,
 *  which is the overwhelmingly common case, still collapse into exactly
 *  one request.
 *
 *  Known limitation: aborting an individual call's `AbortSignal` only
 *  cancels it if the flush hasn't been sent yet — it does not cancel an
 *  in-flight batch. */
export function createBatchLink(options: BatchLinkOptions = {}): RpcLink {
  const maxBatchSize = Math.max(1, options.maxBatchSize ?? Number.POSITIVE_INFINITY);
  const dedupe = options.dedupe ?? defaultDedupe;
  const windowMs = options.windowMs;

  const queue: QueueEntry[] = [];
  let flushScheduled = false;

  function scheduleFlush(): void {
    if (flushScheduled) {
      return;
    }
    flushScheduled = true;
    const run = () => {
      flushScheduled = false;
      flush();
    };
    if (windowMs === undefined) {
      queueMicrotask(run);
    } else {
      setTimeout(run, windowMs);
    }
  }

  function flush(): void {
    if (queue.length === 0) {
      return;
    }
    // Drain the whole queue, then split it. Chunks are dispatched
    // concurrently rather than rescheduled onto later ticks: the set of
    // requests issued is identical either way, and firing them together
    // avoids serializing an oversized fan-in behind its own window.
    const pending = queue.splice(0, queue.length);
    for (const part of partition(pending, options)) {
      for (let i = 0; i < part.entries.length; i += maxBatchSize) {
        void dispatchPartition(part.entries.slice(i, i + maxBatchSize), part.config, dedupe);
      }
    }
  }

  return async (request, next) => {
    if (request.kind === "batch") {
      return next(request);
    }

    return new Promise<RpcLinkResponse>((resolve, reject) => {
      if (request.signal?.aborted) {
        reject(request.signal.reason ?? new Error("aborted"));
        return;
      }

      const entry: QueueEntry = { request, resolve, reject };
      request.signal?.addEventListener("abort", () => {
        const index = queue.indexOf(entry);
        // No-op once already flushed — cancelling one call after its
        // batch has been sent does not cancel the network request.
        if (index !== -1) {
          queue.splice(index, 1);
          reject(request.signal!.reason ?? new Error("aborted"));
        }
      });

      queue.push(entry);
      scheduleFlush();
    });
  };
}
