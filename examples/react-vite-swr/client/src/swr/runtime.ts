// A value import back into `models.ts`, which imports `JsonValue` from
// this file in the other direction. The cycle is fine and already
// established by `rpc-runtime.ts.j2`'s identical import: the other leg is
// `import type`, which is erased entirely at compile time.
import { encodeBinaryAsJson } from "../models.js";
import { COMPACT_JSON_CONTENT_TYPE, RestCompactSession } from "./compact.js";
import type { RestCompactCall } from "./compact.js";

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

// Hex-encoded SHA-256 of the schema's source bytes (issue #178) — baked in
// at generation time from `TypeScriptGeneratorConfig::schema_sha256`. Sent
// as `x-cratestack-schema-sha` on every request so a client compiled
// against a stale `.cstack` schema shows up as a server-side
// `tracing::warn!`, never a rejection. Empty when the CLI wasn't given a
// schema fingerprint (e.g. this crate used as a library directly, or a
// test) — the header is simply omitted in that case.
export const SCHEMA_SHA256: string = "02cc93609f6e33ef57205aa2bda8a1e6ea731cfe9de9468042ff673c83a8e408";
const SCHEMA_SHA_HEADER = "x-cratestack-schema-sha";

// Per-op contract digests (binding version 2, cratestack#1123): what a signed
// request to each op binds into its COSE AAD, keyed like the Rust client's
// `OP_CONTRACTS` (the RPC op id, or "<METHOD> <route template>" on REST, plus
// `batch` for `transport rpc`). Computed by the same `cratestack_core`
// function the Rust macros call, so the two cannot disagree. Not used by the
// unsigned runtime below: the sealers (ADR 0006 §11) read them.
export const OP_CONTRACTS: Readonly<Record<string, string>> = {
  "DELETE /boards/{id}": "22764560f39f87474a551a31a537e48d56b349ed4ed27b18583e3e0cf2a4379a",
  "DELETE /tasks/{id}": "b80b4eedab7e6623b58e33216f67202c49c61f37abff6ebd3c6f96c49d98934c",
  "GET /boards": "89946ea5de7c870e5528bf0babd4be9ffb1322006c755bc92ac77e5cbd4d5c23",
  "GET /boards/{id}": "73effcaf09a18ae5cb6aadac429f179c9d2a3ad798097e9c03ccdfe8413096e6",
  "GET /tasks": "34d7de783bcb93d694e848db3471f9c86c5c9204984efb17373e9c2f6d04079e",
  "GET /tasks/{id}": "3f6748635dac9ff32302ee383c0e891e23e0d7f0bd96cf93edd8f2c80bdcb61a",
  "PATCH /boards/{id}": "48da7b836217709abcf4fa3253c090ee68f2b86189f8ea6240c6cf43975716ea",
  "PATCH /tasks/{id}": "bc5b440b35082a7fa989477693522f48c1cce02101f0c95b4440fddfd394ed49",
  "POST /$procs/clearDoneTasks": "e61cb59a793682d54d88f5bcc3a69dd82f687798307d107cc56ecfc8429f1dde",
  "POST /$procs/estimateFocusMinutes": "a96af61cf2998d8b76c69baf5f907b76860f3eaa5b64cf35134192a406023e51",
  "POST /boards": "3a348af6855d701092188c2f19366818c059a59f6770e28ed7945cd84c12e394",
  "POST /tasks": "35ca0825853c3f3c577159e9c3568612df10c63d3ca6c1b9d825abdda60cb44a",
};
// Hex of the whole-contract digest: moves when any op's contract does.
export const CLIENT_CONTRACT_SHA256: string = "6233d9c56994306cca7cf14e719addb198141a6aa0f3106d951212b6d31ece23";

// Base64 of the compact wire key tables for every op above (the artifact
// `cratestack_core::compact::KeyTables::to_bytes` writes), read only when
// `compactKeys` is on (see `./compact`).
export const KEY_TABLES: string = "gwGJhGJpZABkbmFtZQCEYmlkAGRuYW1lAIplYm9hcmQCZ2JvYXJkSWQAZGRvbmUAYmlkAGV0aXRsZQCCZ2JvYXJkSWQAgmdyZW1vdmVkAIJkYXJncwSEbm1pbnV0ZXNQZXJUYXNrAGl0YXNrQ291bnQAgmx0b3RhbE1pbnV0ZXMAgmRhcmdzB4yEc0RFTEVURSAvYm9hcmRzL3tpZH1YICJ2RWDzn4dHSlUaMaU35I1Ws0ntTtJ7GFg+PgzypDeaAAGEckRFTEVURSAvdGFza3Mve2lkfVgguAtO7at+ZiO1jjMhb2cgLEnGHzer/269PG+WxJ2Yk0wAA4RrR0VUIC9ib2FyZHNYIImUbqXefIcOVSi/C6vUvp/7EyIAbHVbySrHfly9TVwjAIIAAYRwR0VUIC9ib2FyZHMve2lkfVggc+/8rwmhiuXLaq2sQp8XnJ0qOteYCX6cA8zf6EEwluYAAYRqR0VUIC90YXNrc1ggNNfeeDvLk9aU6EjbNHH5yGxckgSYTvsXNz6cL20EB54AggADhG9HRVQgL3Rhc2tzL3tpZH1YID9nSGNdrJ/zIwLuODwOiR4j4NfwvZbPk+3Y8sgL3LYaAAOEclBBVENIIC9ib2FyZHMve2lkfVggSNp7g2IXcJq89PoyU8CQ7mjyuGGJ+OpiQMbPQ5dXFuoBAYRxUEFUQ0ggL3Rhc2tzL3tpZH1YILxbRAs1CCp/qYlHdpNSL0jBzOAhAfDJW0RA/d/TlO1JAwOEeBtQT1NUIC8kcHJvY3MvY2xlYXJEb25lVGFza3NYIOYctZp5NoLVTYj1vMOmndgvaHeYMH0QfMVuz8hCnx3eBgWEeCFQT1NUIC8kcHJvY3MvZXN0aW1hdGVGb2N1c01pbnV0ZXNYIKlq9hzymY2Ldsabr1+Qe3aGDz6qW2TPNRNBkqQGAj5RCQiEbFBPU1QgL2JvYXJkc1ggOjSK9oVdcBCSGIwvGTZoGMBZpZ9ncOKO15Rc2EwS45QBAYRrUE9TVCAvdGFza3NYIDXKCCWFPD88V3FZ6cNWhhLfEMY9PKbBudglq92mDLRKAwM=";

export interface CratestackClientOptions {
  basePath?: string;
  fetch?: typeof fetch;
  headers?: HeadersInit | (() => HeadersInit | Promise<HeadersInit>);
  /** Opt in to compact wire keys: schema-declared fields travel as integer
   *  keys (`application/vnd.cratestack.compact+json`). Default `false`, which
   *  leaves the wire byte-identical to a client without the option. The
   *  request body goes compact only after the op has answered compact once,
   *  so an older server is never sent a body it cannot read. Loads the
   *  optional peer `@cratestack/cbor` on first use; without it, or with a
   *  release that predates `createCompactKeys`, the option is silently
   *  inert. Error bodies stay named. */
  compactKeys?: boolean;
}

/** Serializable request prepared before its first network attempt. */
export interface PreparedCall {
  version: 1;
  method: string;
  path: string;
  url: string;
  op: string;
  contract: string;
  keyTables: string;
  headers: [string, string][];
  body: string | null;
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

const RESERVED_ERROR_MEMBERS: ReadonlySet<string> = new Set(["code", "message", "details"]);

/** The members of a decoded error body beyond `code`, `message` and
 *  `details`: what an `AuthProvider` refusal adds (cratestack#1208), such
 *  as `{ error: "attestation_expired" }`. Every member is kept, as decoded;
 *  empty when `body` is not an object. */
function extraErrorMembers(body: unknown): Readonly<Record<string, unknown>> {
  const extra: Record<string, unknown> = {};
  if (typeof body === "object" && body !== null && !Array.isArray(body)) {
    for (const [name, value] of Object.entries(body)) {
      if (!RESERVED_ERROR_MEMBERS.has(name)) {
        extra[name] = value;
      }
    }
  }
  return Object.freeze(extra);
}

/** The answer's `WWW-Authenticate` value, raw and unparsed (cratestack#1243);
 *  empty when there is none. `Headers` joins two challenge lines with `, `,
 *  so they arrive as one entry holding both. */
function challengesOf(headers: Headers | undefined): readonly string[] {
  const value = headers?.get("www-authenticate");
  return Object.freeze(value ? [value] : []);
}

/** The extra member `error` of a step-up refusal (RFC 9470, cratestack#1244). */
export const INSUFFICIENT_USER_AUTHENTICATION = "insufficient_user_authentication";

/** What a step-up refusal asks for (cratestack#1244): an operation that
 *  requires a fresher or stronger authentication than the caller presented.
 *  Each requirement is present only when the refused operation declares it. */
export interface StepUpRequired {
  /** The longest time, in seconds, since the caller last authenticated. */
  readonly maxAge: number | undefined;
  /** The authentication context classes the operation accepts. */
  readonly acrValues: readonly string[];
  /** The authentication methods the operation requires. */
  readonly amr: readonly string[];
  /** Which requirements the caller failed: a subset of `"max_age"`, `"acr"`
   *  and `"amr"`. */
  readonly unmet: readonly string[];
}

/** Reads a step-up refusal once the transport's own test (`refused`: a 401,
 *  or `unauthenticated`) passed: `undefined` unless the extra member `error`
 *  is `insufficient_user_authentication`. Never throws: a missing or
 *  mistyped field reads as absent or empty. */
function readStepUp(
  refused: boolean,
  details: unknown,
  extra: Readonly<Record<string, unknown>>,
): StepUpRequired | undefined {
  if (!refused || extra["error"] !== INSUFFICIENT_USER_AUTHENTICATION) {
    return undefined;
  }
  const map: Record<string, unknown> =
    typeof details === "object" && details !== null && !Array.isArray(details)
      ? (details as Record<string, unknown>)
      : {};
  const strings = (name: string): readonly string[] => {
    const value = map[name];
    return Object.freeze(
      Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [],
    );
  };
  const raw = map["max_age"];
  const maxAge = typeof raw === "bigint" ? Number(raw) : raw;
  return Object.freeze({
    maxAge:
      typeof maxAge === "number" && Number.isSafeInteger(maxAge) && maxAge >= 0
        ? maxAge
        : undefined,
    acrValues: strings("acr_values"),
    amr: strings("amr"),
    unmet: strings("unmet"),
  });
}

/** Runs `call`; when it fails with a step-up refusal, hands the refusal to
 *  `reprove` and, when that resolves `true` (the user authenticated again),
 *  runs `call` **once** more and returns what it gives, even a second
 *  refusal. Never loops (cratestack#1244).
 *
 *  `reprove` resolves `false` when the user declined or the re-proof failed;
 *  the first refusal is then rethrown. Any other error, and one `reprove`
 *  itself throws, propagates as it is.
 *
 *  ```ts
 *  const receipt = await retryAfterStepUp(
 *    () => client.procedures.transfer(args),
 *    (stepUp) => session.reauthenticate({ maxAge: stepUp.maxAge }),
 *  );
 *  ``` */
export async function retryAfterStepUp<T>(
  call: () => Promise<T>,
  reprove: (stepUp: StepUpRequired) => Promise<boolean>,
): Promise<T> {
  try {
    return await call();
  } catch (error) {
    const stepUp = stepUpOf(error);
    if (stepUp === undefined || !(await reprove(stepUp))) {
      throw error;
    }
  }
  return call();
}

/** The error envelope a CrateStack server answers with. Mirrors
 *  `cratestack_core::CratestackErrorResponse`; the members an
 *  `AuthProvider` refusal adds beside these three are on
 *  {@link CratestackHttpError.extra}. */
export interface CratestackErrorResponse {
  code: string;
  message: string;
  details?: unknown;
}

function isErrorResponse(payload: unknown): payload is CratestackErrorResponse {
  return (
    typeof payload === "object" &&
    payload !== null &&
    typeof (payload as { code?: unknown }).code === "string"
  );
}

/** Thrown when the server answers with a non-2xx status. `payload` is the
 *  whole decoded body; the envelope's members are typed beside it: `body`
 *  (`code`, `message`, `details`), `extra` (every other member, as an
 *  `AuthProvider` refusal adds them) and `challenges` (the
 *  `WWW-Authenticate` value). */
export class CratestackHttpError extends Error {
  readonly status: number;
  readonly response: Response;
  readonly payload: unknown;
  /** The decoded envelope, or `undefined` when the payload is not one. */
  readonly body: CratestackErrorResponse | undefined;
  /** The envelope's `code` (`UNAUTHORIZED`, `FORBIDDEN`, ...). */
  readonly code: string | undefined;
  readonly details: unknown;
  readonly extra: Readonly<Record<string, unknown>>;
  readonly challenges: readonly string[];
  /** The step-up refusal this answer is (cratestack#1244): a 401 whose
   *  extra member `error` is `insufficient_user_authentication`, read from
   *  `details`; `undefined` for any other error. */
  readonly stepUp: StepUpRequired | undefined;

  constructor(response: Response, payload: unknown) {
    super(`CrateStack request failed with status ${response.status}`);
    this.name = "CratestackHttpError";
    this.status = response.status;
    this.response = response;
    this.payload = payload;
    this.body = isErrorResponse(payload) ? payload : undefined;
    this.code = this.body?.code;
    this.details = this.body?.details;
    this.extra = extraErrorMembers(this.body);
    this.challenges = challengesOf(response.headers);
    this.stepUp = readStepUp(this.status === 401, this.details, this.extra);
  }
}

/** The step-up refusal of a {@link CratestackHttpError} (its `stepUp`);
 *  `undefined` for anything else (cratestack#1244). */
export function stepUpOf(error: unknown): StepUpRequired | undefined {
  return error instanceof CratestackHttpError ? error.stepUp : undefined;
}

export class CratestackRuntime {
  readonly origin: string;
  readonly basePath: string;
  readonly fetchFn: typeof fetch;
  readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;
  private readonly compact: RestCompactSession | undefined;

  constructor(origin: string, options: CratestackClientOptions = {}) {
    this.origin = trimTrailingSlashes(origin);
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
    this.compact =
      options.compactKeys === true
        ? new RestCompactSession(KEY_TABLES, Object.keys(OP_CONTRACTS))
        : undefined;
  }

  /** Prepare a keyed request for caller-owned durable storage before sending. */
  async prepare(method: string, path: string, options: CratestackRequestOptions = {}): Promise<PreparedCall> {
    if (!["POST", "PUT", "PATCH", "DELETE"].includes(method.toUpperCase())) throw new Error("prepare requires a mutation");
    const target = new URL(this.url(path, options.query));
    if (target.username || target.password) throw new Error("prepared URL must not contain userinfo");
    const headers = new Headers(await resolveHeaders(this.defaultHeaders));
    for (const [key, value] of new Headers(options.headers)) headers.set(key, value);
    if (!headers.get("Idempotency-Key")) throw new Error("prepare requires an Idempotency-Key");
    const routes = this.compact ?? new RestCompactSession(KEY_TABLES, Object.keys(OP_CONTRACTS));
    const op = routes.opOf(method, path);
    const contract = op === undefined ? undefined : OP_CONTRACTS[op];
    if (op === undefined || contract === undefined) throw new Error("unknown prepared route");
    let body = options.body === undefined ? null : JSON.stringify(encodeBinaryAsJson(options.body));
    headers.set("Accept", "application/json");
    if (body !== null) headers.set("Content-Type", "application/json");
    const compact = await this.compact?.begin(method, path, headers.get("Idempotency-Key") ?? undefined);
    if (compact !== undefined) {
      compact.applyHeaders(headers, body !== null);
      if (body !== null) body = compact.encodeBody(body);
    }
    if (body !== null && new TextEncoder().encode(body).length > 8 * 1024 * 1024) throw new Error("prepared body too large");
    for (const name of [...headers.keys()]) if (!["idempotency-key", "if-match", "accept", "content-type", "cratestack-contract"].includes(name)) headers.delete(name);
    const prepared: PreparedCall = { version: 1, method: method.toUpperCase(), path, url: target.toString(), op, contract, keyTables: KEY_TABLES, headers: [...headers], body };
    if (new TextEncoder().encode(JSON.stringify(prepared)).length > 16 * 1024 * 1024) throw new Error("prepared call too large");
    return prepared;
  }

  async executePrepared<T>(prepared: PreparedCall, signal?: AbortSignal): Promise<T> {
    const serialized = JSON.stringify(prepared);
    if (new TextEncoder().encode(serialized).length > 16 * 1024 * 1024) throw new Error("prepared call too large");
    prepared = JSON.parse(serialized) as PreparedCall;
    if (Object.keys(prepared).sort().join(",") !== "body,contract,headers,keyTables,method,op,path,url,version" || prepared.version !== 1 ||
      typeof prepared.method !== "string" || typeof prepared.path !== "string" || typeof prepared.url !== "string" ||
      typeof prepared.op !== "string" || typeof prepared.contract !== "string" || prepared.contract !== OP_CONTRACTS[prepared.op] || prepared.keyTables !== KEY_TABLES ||
      !Array.isArray(prepared.headers) || (prepared.body !== null && typeof prepared.body !== "string")) {
      throw new Error("invalid or incompatible prepared call");
    }
    const target = new URL(prepared.url);
    if (target.username || target.password) throw new Error("prepared URL must not contain userinfo");
    if (prepared.body !== null) JSON.parse(prepared.body);
    const names = new Set<string>();
    for (const pair of prepared.headers) {
      if (!Array.isArray(pair) || pair.length !== 2 || typeof pair[0] !== "string" || typeof pair[1] !== "string") throw new Error("invalid prepared header");
      const name = pair[0].toLowerCase();
      if (names.has(name) || !["idempotency-key", "if-match", "accept", "content-type", "cratestack-contract"].includes(name) || new TextEncoder().encode(pair[1]).length > 8192) throw new Error("invalid prepared header");
      names.add(name);
    }
    const routes = this.compact ?? new RestCompactSession(KEY_TABLES, Object.keys(OP_CONTRACTS));
    if (!["POST", "PUT", "PATCH", "DELETE"].includes(prepared.method) || routes.opOf(prepared.method, prepared.path) !== prepared.op ||
      this.url(prepared.path).split("?")[0] !== prepared.url.split("?")[0] ||
      (prepared.body !== null && new TextEncoder().encode(prepared.body).length > 8 * 1024 * 1024)) throw new Error("invalid prepared route or body");
    const saved = new Headers(prepared.headers);
    if (!saved.get("Idempotency-Key")) throw new Error("invalid prepared key");
    const headers = new Headers(await resolveHeaders(this.defaultHeaders));
    if (SCHEMA_SHA256 !== "") headers.set(SCHEMA_SHA_HEADER, SCHEMA_SHA256);
    for (const name of ["idempotency-key", "if-match", "accept", "content-type", "cratestack-contract"]) headers.delete(name);
    for (const [key, value] of saved) headers.set(key, value);
    const compact = await this.compact?.begin(prepared.method, prepared.path, headers.get("Idempotency-Key") ?? undefined);
    if (headers.has("Cratestack-Contract") && compact === undefined) throw new Error("compact decoder unavailable");
    const expected = new Headers();
    if (compact !== undefined) compact.applyHeaders(expected, prepared.body !== null);
    const contentType = saved.get("content-type");
    if (saved.get("accept") !== (compact ? expected.get("accept") : "application/json") ||
        saved.get("cratestack-contract") !== expected.get("cratestack-contract") ||
        (prepared.body !== null && contentType !== "application/json" && contentType !== "application/vnd.cratestack.compact+json") ||
        (contentType === "application/vnd.cratestack.compact+json" && compact === undefined)) throw new Error("invalid prepared media or selector");

    const response = await this.fetchFn(prepared.url, { method: prepared.method, headers, body: prepared.body, signal: signal ?? null });
    const payload = await readResponsePayload(response, compact);
    if (!response.ok) throw new CratestackHttpError(response, payload);
    return payload as T;
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

    const effectiveHeaders = new Headers(headers);
    for (const [key, value] of new Headers(options.headers)) effectiveHeaders.set(key, value);
    const compact = await this.compact?.begin(method, path, effectiveHeaders.get("Idempotency-Key") ?? undefined);
    let body: BodyInit | undefined;
    if (options.body !== undefined) {
      headers.set("Content-Type", "application/json");
      // `encodeBinaryAsJson` rewrites every `Uint8Array` (a `Bytes`
      // field's client-side type) into the integer array the wire uses —
      // `JSON.stringify` alone turns one into an index-keyed object no
      // server-side `Vec<u8>` can decode — and every `bigint` (a `BigInt`
      // field) into its canonical decimal string, which `JSON.stringify`
      // alone refuses to serialize at all. Unconditional here, unlike the
      // RPC transport's codec-dependent handling, because a REST request
      // body is always JSON.
      body = JSON.stringify(encodeBinaryAsJson(options.body));
    }
    if (compact !== undefined) {
      compact.applyHeaders(headers, body !== undefined);
      if (typeof body === "string") {
        body = compact.encodeBody(body);
      }
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

    if (!response.ok) {
      // A `415` sends the op back to named bodies (see `./compact`).
      compact?.noteFailure(response.status);
    }
    const payload = await readResponsePayload(response, compact);
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
    const normalizedBase = trimTrailingSlashes(this.basePath);
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

async function readResponsePayload(
  response: Response,
  compact?: RestCompactCall,
): Promise<unknown> {
  if (response.status === 204) {
    return undefined;
  }

  const text = await response.text();
  if (text.length === 0) {
    return undefined;
  }

  const contentType = response.headers.get("Content-Type") ?? "";
  // A compact body is transcoded back to plain JSON; an error body never is.
  if (
    compact !== undefined &&
    response.ok &&
    contentType.split(";", 1)[0]?.trim() === COMPACT_JSON_CONTENT_TYPE
  ) {
    return JSON.parse(compact.decodeResponse(text));
  }
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
    // The same walk the body gets: an object-valued query entry
    // (`computedParams`) may hold a `bigint`, which `JSON.stringify` throws
    // on. A scalar `bigint` below needs nothing: `String(7n)` is `"7"`.
    searchParams.set(key, JSON.stringify(encodeBinaryAsJson(value)));
    return;
  }

  searchParams.append(key, String(value));
}

function trimTrailingSlashes(value: string): string {
  let end = value.length;
  while (end > 0 && value.charCodeAt(end - 1) === 47) {
    end--;
  }
  return value.slice(0, end);
}