import axios, { type AxiosInstance, type AxiosRequestConfig, type Method } from "axios";

// The Fetch API's `Response` constructor throws if given a non-null body
// alongside one of these statuses (https://fetch.spec.whatwg.org/#null-body-status)
// — even an empty `ArrayBuffer` counts as "non-null". axios always hands
// back a `data` buffer regardless of status, so it has to be dropped
// explicitly here rather than passed through.
const NULL_BODY_STATUSES = new Set([204, 205, 304]);

/** Options for {@link createAxiosRuntime}. */
export interface AxiosRuntimeOptions {
  /** Axios instance to issue requests through — e.g. one preconfigured
   *  with interceptors, a base URL, or a custom `httpsAgent`. Defaults
   *  to the top-level `axios` singleton. */
  instance?: AxiosInstance;
}

/** Adapts an axios instance to the `typeof fetch` signature so it can be
 *  passed as `CratestackRpcClientOptions.fetch` (the generated runtime's
 *  constructor option) or as an `RpcLink`'s `request.fetchFn` override —
 *  axios becomes the actual transport while every link and the
 *  generated runtime keep speaking the Fetch API's `Request`/`Response`
 *  shape, unaware anything changed underneath. */
export function createAxiosRuntime(options: AxiosRuntimeOptions = {}): typeof fetch {
  const instance = options.instance ?? axios;

  return (async (input, init) => {
    const resolved = await resolveRequest(input, init);
    const config: AxiosRequestConfig = {
      url: resolved.url,
      method: resolved.method as Method,
      headers: headersToObject(resolved.headers),
      data: resolved.body,
      // Built via conditional spread, not `signal: resolved.signal ??
      // undefined` — with `exactOptionalPropertyTypes: true`, explicitly
      // assigning `undefined` to `signal` isn't the same as omitting the
      // key, and axios's `GenericAbortSignal` type (unlike `data`, which
      // is `any`) rejects it.
      ...(resolved.signal ? { signal: resolved.signal } : {}),
      responseType: "arraybuffer",
      // The generated runtime and every `RpcLink` decide success/failure
      // from `response.status`/`response.ok` themselves (see
      // `CratestackRpcRuntime.readUnaryResponse`) — axios's default of
      // throwing on a non-2xx status would short-circuit that and turn
      // every server-side `RpcErrorBody` into a thrown `AxiosError`
      // instead of a normal `Response` the caller can inspect.
      validateStatus: () => true,
    };

    const response = await instance.request(config);

    return new Response(
      NULL_BODY_STATUSES.has(response.status) ? null : (response.data as ArrayBuffer),
      {
        status: response.status,
        statusText: response.statusText,
        headers: axiosHeadersToFetchHeaders(response.headers),
      },
    );
  }) as typeof fetch;
}

interface ResolvedRequest {
  url: string;
  method: string;
  headers: HeadersInit | undefined;
  body: BodyInit | null | undefined;
  signal: AbortSignal | undefined;
}

/** The generated runtime only ever calls `fetchFn(url, init)` with a
 *  string URL and a populated `init` — this function's `Request`
 *  handling exists purely for `typeof fetch` compatibility with callers
 *  that pass a pre-built `Request` (e.g. from another link/middleware).
 *  `init`'s fields, when present, override the `Request`'s own — same
 *  precedence the real Fetch API gives `fetch(request, init)`. */
async function resolveRequest(
  input: RequestInfo | URL,
  init: RequestInit | undefined,
): Promise<ResolvedRequest> {
  if (typeof input === "string" || input instanceof URL) {
    return {
      url: input.toString(),
      method: init?.method ?? "GET",
      headers: init?.headers,
      body: init?.body ?? undefined,
      signal: init?.signal ?? undefined,
    };
  }
  return {
    url: input.url,
    method: init?.method ?? input.method,
    headers: init?.headers ?? input.headers,
    body:
      init?.body !== undefined
        ? init.body
        : input.body !== null
          ? await input.clone().arrayBuffer()
          : undefined,
    signal: init?.signal ?? input.signal,
  };
}

function headersToObject(headers: HeadersInit | undefined): Record<string, string> {
  const result: Record<string, string> = {};
  if (!headers) {
    return result;
  }
  // `.forEach()` (rather than `.entries()`/`for...of`) is the one
  // iteration style declared on `Headers` consistently across both the
  // DOM lib type and the Node/undici one that `axios`'s own type
  // dependencies can pull into scope — the other two aren't.
  new Headers(headers).forEach((value, key) => {
    result[key] = value;
  });
  return result;
}

function axiosHeadersToFetchHeaders(headers: unknown): Headers {
  const result = new Headers();
  if (typeof headers !== "object" || headers === null) {
    return result;
  }
  for (const [key, value] of Object.entries(headers as Record<string, unknown>)) {
    if (value === undefined) {
      continue;
    }
    result.set(key, Array.isArray(value) ? value.join(", ") : String(value));
  }
  return result;
}
