import type {
  RpcErrorBody,
  RpcLink,
  RpcLinkRequest,
  RpcLinkResponse,
  RpcRequest,
  RpcResponseFrame,
} from "@cratestack/ts-types";

export interface BatchLinkOptions {
  /** Scheduling window. Omitted (default) uses `queueMicrotask` — calls
   *  fired synchronously in the same tick (e.g. inside `Promise.all`)
   *  collapse into one `/rpc/batch` request. Pass a millisecond value to
   *  widen the window across ticks (e.g. debounce bursts from unrelated
   *  components). */
  windowMs?: number;
  /** Maximum requests per flushed `/rpc/batch` call. Default: unbounded
   *  — the server enforces no size limit either (see
   *  `docs/design/rpc-transport.md` §3.2), but a huge fan-in may still
   *  be worth capping client-side. */
  maxBatchSize?: number;
  /** Returns a dedupe key for a queued call, or `null` to never
   *  collapse it with anything else. Two calls in the same flush that
   *  return the same non-null key become ONE `/rpc/batch` frame, and
   *  every caller resolves from that single result.
   *
   *  Default: only collapse calls that share an explicit
   *  `idempotencyKey` — that's already the caller's own signal that the
   *  call is safe to treat as a repeat. Calls with no idempotency key
   *  are never auto-collapsed, since the server does no dedup of its
   *  own and silently merging two textually-identical but unmarked
   *  mutations would be unsafe. Pass a custom `dedupe` for full
   *  value-based batshit-style collapsing (e.g.
   *  `req => \`${req.opId}:${JSON.stringify(req.input)}\``), or
   *  `() => null` to disable dedup entirely and only batch.
   */
  dedupe?: (request: RpcLinkRequest) => string | null;
}

interface QueueEntry {
  readonly request: RpcLinkRequest;
  readonly resolve: (value: RpcLinkResponse) => void;
  readonly reject: (reason: unknown) => void;
}

interface Group {
  readonly key: string | null;
  readonly entries: QueueEntry[];
}

const defaultDedupe = (request: RpcLinkRequest): string | null =>
  request.idempotencyKey !== undefined ? `idem:${request.idempotencyKey}` : null;

/** A batshit-style (github.com/yornaath/batshit) automatic batch
 *  scheduler, shipped as an `RpcLink` (issue #182's composition
 *  mechanism) rather than a `fetch` override — so it composes with a
 *  logger, retry, or auth-refresh link instead of clobbering them.
 *
 *  Terminal in practice: for `kind: "unary"` calls it never invokes
 *  `next` — it queues the call and later performs its own single
 *  `POST /rpc/batch`. An explicit `runtime.batch()` call (`kind:
 *  "batch"`) passes straight through via `next`, since it's already
 *  the shape this link would otherwise build.
 *
 *  Known limitation: the synthesized `/rpc/batch` request reuses the
 *  first queued call's `headers`/`fetchFn`/`codec` for the whole flush
 *  — per-call custom headers on later calls in the same window are not
 *  applied to the aggregate request. Pass shared headers via the
 *  runtime's own `headers` option rather than per-call
 *  `CratestackRpcCallOptions.headers` when using this link. Similarly,
 *  aborting an individual call's `AbortSignal` only cancels it if the
 *  flush hasn't been sent yet — it does not cancel an in-flight batch. */
export function createBatchLink(options: BatchLinkOptions = {}): RpcLink {
  const maxBatchSize = options.maxBatchSize ?? Number.POSITIVE_INFINITY;
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
    const size = Math.max(1, maxBatchSize);
    const batch = queue.splice(0, size);
    if (queue.length > 0) {
      scheduleFlush();
    }
    void runBatch(batch);
  }

  async function runBatch(entries: QueueEntry[]): Promise<void> {
    const groups = groupByDedupeKey(entries, dedupe);
    const leader = entries[0]!.request;

    const requests: RpcRequest[] = groups.map((group, id) => {
      const request = group.entries[0]!.request;
      const frame: RpcRequest = { id, op: request.opId, input: request.input };
      if (request.idempotencyKey !== undefined) {
        frame.idem = request.idempotencyKey;
      }
      return frame;
    });

    try {
      const response = await leader.fetchFn(leader.urls.batch(), {
        method: "POST",
        headers: leader.headers,
        body: leader.codec.encode(requests),
        signal: null,
      });

      if (!response.ok) {
        const bytes = new Uint8Array(await response.arrayBuffer().catch(() => new ArrayBuffer(0)));
        for (const group of groups) {
          for (const entry of group.entries) {
            entry.resolve({ response: new Response(bytes, { status: response.status }) });
          }
        }
        return;
      }

      const bytes = new Uint8Array(await response.arrayBuffer());
      const frames = leader.codec.decode(bytes) as RpcResponseFrame[];
      resolveGroups(groups, frames);
    } catch (error) {
      for (const group of groups) {
        for (const entry of group.entries) {
          entry.reject(error);
        }
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

function groupByDedupeKey(
  entries: QueueEntry[],
  dedupe: (request: RpcLinkRequest) => string | null,
): Group[] {
  const groups: Group[] = [];
  const indexByKey = new Map<string, number>();
  for (const entry of entries) {
    const key = dedupe(entry.request);
    if (key === null) {
      groups.push({ key: null, entries: [entry] });
      continue;
    }
    const existingIndex = indexByKey.get(key);
    if (existingIndex === undefined) {
      indexByKey.set(key, groups.length);
      groups.push({ key, entries: [entry] });
    } else {
      groups[existingIndex]!.entries.push(entry);
    }
  }
  return groups;
}

function resolveGroups(groups: Group[], frames: RpcResponseFrame[]): void {
  // Correlate by `id`, not array position — the server contract
  // guarantees order (docs/design/rpc-transport.md §3.2), but matching
  // by id is strictly more robust and is the pattern the repo's own
  // Rust client-side batch debouncer already establishes
  // (examples/rpc-batch-debounce/src/lib.rs's `responders` map).
  const frameById = new Map(frames.map((frame) => [frame.id, frame]));
  groups.forEach((group, id) => {
    const frame = frameById.get(id);
    if (!frame) {
      const error = new Error(`batch response is missing frame id ${id}`);
      for (const entry of group.entries) {
        entry.reject(error);
      }
      return;
    }

    const hasError = frame.error !== undefined;
    for (const entry of group.entries) {
      if (!hasError && frame.output === undefined) {
        // Mirrors the unary path's `response.status === 204` shortcut
        // (void-returning calls resolve `undefined`, not a decoded null).
        entry.resolve({ response: new Response(null, { status: 204 }) });
        continue;
      }
      const body: RpcErrorBody | unknown = hasError ? frame.error : frame.output;
      entry.resolve({
        response: new Response(entry.request.codec.encode(body), {
          status: hasError ? errorStatus(frame.error!.code) : 200,
        }),
      });
    }
  });
}

function errorStatus(code: string): number {
  switch (code) {
    case "invalid_argument":
    case "failed_precondition":
      return 400;
    case "unauthenticated":
      return 401;
    case "permission_denied":
      return 403;
    case "not_found":
      return 404;
    case "conflict":
      return 409;
    default:
      return 500;
  }
}
