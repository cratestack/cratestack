/** Options for {@link createFetchRuntime}. */
export interface FetchRuntimeOptions {
  /** Underlying `fetch` to wrap. Defaults to the global `fetch`. */
  fetch?: typeof fetch;
  /** Aborts a call that hasn't settled within this many milliseconds.
   *  Combined with any `AbortSignal` the caller already passed in
   *  `init.signal` — either firing aborts the request. Omitted (default)
   *  applies no timeout at all, matching plain `fetch`. */
  timeoutMs?: number;
}

/** A `typeof fetch`-compatible transport — pass it as
 *  `CratestackRpcClientOptions.fetch` (the generated runtime's
 *  constructor option) or as an `RpcLink`'s `request.fetchFn` override.
 *  With no options this is a byte-identical pass-through to the global
 *  `fetch`; `timeoutMs` is the only behavior it adds. */
export function createFetchRuntime(options: FetchRuntimeOptions = {}): typeof fetch {
  const baseFetch = options.fetch ?? fetch;
  const timeoutMs = options.timeoutMs;
  if (timeoutMs === undefined) {
    return baseFetch;
  }

  return (async (input, init) => {
    const timeoutSignal = AbortSignal.timeout(timeoutMs);
    const signal = init?.signal ? combineSignals(init.signal, timeoutSignal) : timeoutSignal;
    return baseFetch(input, { ...init, signal });
  }) as typeof fetch;
}

/** Node 18 (this package's minimum) has no `AbortSignal.any` — that
 *  landed in Node 20 / evergreen browsers around the same time. Combines
 *  by hand instead of assuming it's available. */
function combineSignals(a: AbortSignal, b: AbortSignal): AbortSignal {
  if (a.aborted) {
    return a;
  }
  if (b.aborted) {
    return b;
  }
  const controller = new AbortController();
  a.addEventListener("abort", () => controller.abort(a.reason), { once: true });
  b.addEventListener("abort", () => controller.abort(b.reason), { once: true });
  return controller.signal;
}
