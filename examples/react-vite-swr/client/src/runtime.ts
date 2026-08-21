export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

// Hex-encoded SHA-256 of the schema's source bytes (issue #178) — baked in
// at generation time from `TypeScriptGeneratorConfig::schema_sha256`. Sent
// as `x-cratestack-schema-sha` on every request so a client compiled
// against a stale `.cstack` schema shows up as a server-side
// `tracing::warn!`, never a rejection. Empty when the CLI wasn't given a
// schema fingerprint (e.g. this crate used as a library directly, or a
// test) — the header is simply omitted in that case.
export const SCHEMA_SHA256: string = "c4ddba2afd01d6174a0e70f0dfedec8591016d4603ca6f4ad773228b5224e3e3";
const SCHEMA_SHA_HEADER = "x-cratestack-schema-sha";

export interface CratestackClientOptions {
  basePath?: string;
  fetch?: typeof fetch;
  headers?: HeadersInit | (() => HeadersInit | Promise<HeadersInit>);
}

export interface CratestackRequestOptions {
  body?: unknown;
  headers?: HeadersInit | undefined;
  query?: Record<string, unknown> | undefined;
  signal?: AbortSignal | undefined;
}

// Issue #610: `request()` decodes the body and discards the `Response`,
// so no caller can ever reach a response header (`ETag`, most notably —
// the generated server stamps it on every `@version` model's GET/detail
// response, and requires it back as `If-Match` on PATCH/DELETE). This
// envelope is what `requestWithResponse()`/`getWithResponse()` return
// instead, so the header becomes reachable without changing `request()`'s
// existing return shape for every other call site.
export interface CratestackResponseEnvelope<T> {
  value: T;
  response: Response;
}

export class CratestackHttpError extends Error {
  readonly status: number;
  readonly response: Response;
  readonly payload: unknown;

  constructor(response: Response, payload: unknown) {
    super(`CrateStack request failed with status ${response.status}`);
    this.name = "CratestackHttpError";
    this.status = response.status;
    this.response = response;
    this.payload = payload;
  }
}

export class CratestackRuntime {
  readonly origin: string;
  readonly basePath: string;
  readonly fetchFn: typeof fetch;
  readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;

  constructor(origin: string, options: CratestackClientOptions = {}) {
    this.origin = origin.replace(/\/+$/, "");
    this.basePath = options.basePath ?? "/api";
    // `.bind(globalThis)`, not the bare global — some browsers' `fetch`
    // is spec'd to throw `TypeError: Illegal invocation` when called
    // with a receiver other than the global object (verified for real:
    // storing the bare function on `this` and calling it as
    // `this.fetchFn(...)` reproduces exactly that in Chrome/Vite dev,
    // even though the same code runs fine under Node's `fetch`, which
    // is why this was never caught by a Node-only test). A caller-
    // supplied `options.fetch` is trusted to already be correctly bound.
    this.fetchFn = options.fetch ?? fetch.bind(globalThis);
    this.defaultHeaders = options.headers;
  }

  async request<T>(
    method: string,
    path: string,
    options: CratestackRequestOptions = {},
  ): Promise<T> {
    const { value } = await this.requestWithResponse<T>(method, path, options);
    return value;
  }

  // Same request as `request()`, but returns the `Response` alongside the
  // decoded value (issue #610) instead of discarding it — `request()` is
  // now a thin wrapper around this that keeps only `.value`, so every
  // existing call site's return shape is unchanged.
  async requestWithResponse<T>(
    method: string,
    path: string,
    options: CratestackRequestOptions = {},
  ): Promise<CratestackResponseEnvelope<T>> {
    const headers = new Headers(await resolveHeaders(this.defaultHeaders));
    if (SCHEMA_SHA256 !== "") {
      headers.set(SCHEMA_SHA_HEADER, SCHEMA_SHA256);
    }
    headers.set("Accept", "application/json");

    let body: BodyInit | undefined;
    if (options.body !== undefined) {
      headers.set("Content-Type", "application/json");
      body = JSON.stringify(options.body);
    }

    for (const [key, value] of new Headers(options.headers)) {
      headers.set(key, value);
    }

    const response = await this.fetchFn(this.url(path, options.query), {
      method,
      headers,
      body: body ?? null,
      signal: options.signal ?? null,
    });

    const payload = await readResponsePayload(response);
    if (!response.ok) {
      throw new CratestackHttpError(response, payload);
    }
    return { value: payload as T, response };
  }

  get<T>(path: string, options: Omit<CratestackRequestOptions, "body"> = {}): Promise<T> {
    return this.request<T>("GET", path, options);
  }

  // Issue #610: the READ half of the ETag/If-Match round trip — read
  // `.response.headers.get("etag")` off the result, then pass that value
  // as `ifMatch` to a generated model's `update`/`delete` method.
  getWithResponse<T>(
    path: string,
    options: Omit<CratestackRequestOptions, "body"> = {},
  ): Promise<CratestackResponseEnvelope<T>> {
    return this.requestWithResponse<T>("GET", path, options);
  }

  post<T>(path: string, body: unknown, options: Omit<CratestackRequestOptions, "body"> = {}): Promise<T> {
    return this.request<T>("POST", path, { ...options, body });
  }

  patch<T>(path: string, body: unknown, options: Omit<CratestackRequestOptions, "body"> = {}): Promise<T> {
    return this.request<T>("PATCH", path, { ...options, body });
  }

  delete<T>(path: string, options: Omit<CratestackRequestOptions, "body"> = {}): Promise<T> {
    return this.request<T>("DELETE", path, options);
  }

  private url(path: string, query?: Record<string, unknown>): string {
    const normalizedBase = this.basePath === "/" ? "" : this.basePath.replace(/\/+$/, "");
    const normalizedPath = path.startsWith("/") ? path : `/${path}`;
    const url = new URL(`${normalizedBase}${normalizedPath}`, `${this.origin}/`);
    appendQuery(url.searchParams, query);
    return url.toString();
  }
}

async function resolveHeaders(
  headers: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined,
): Promise<HeadersInit | undefined> {
  if (typeof headers === "function") {
    return headers();
  }
  return headers;
}

async function readResponsePayload(response: Response): Promise<unknown> {
  if (response.status === 204) {
    return undefined;
  }

  const text = await response.text();
  if (text.length === 0) {
    return undefined;
  }

  const contentType = response.headers.get("Content-Type") ?? "";
  if (contentType.includes("application/json")) {
    return JSON.parse(text);
  }
  return text;
}

function appendQuery(searchParams: URLSearchParams, query?: Record<string, unknown>): void {
  if (!query) {
    return;
  }

  for (const [key, value] of Object.entries(query)) {
    appendQueryValue(searchParams, key, value);
  }
}

function appendQueryValue(searchParams: URLSearchParams, key: string, value: unknown): void {
  if (value === undefined || value === null) {
    return;
  }

  if (Array.isArray(value)) {
    for (const item of value) {
      appendQueryValue(searchParams, key, item);
    }
    return;
  }

  if (typeof value === "object") {
    searchParams.set(key, JSON.stringify(value));
    return;
  }

  searchParams.append(key, String(value));
}