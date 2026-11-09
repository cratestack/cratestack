// Sealing a call at the `fetch` boundary (issue #1010, ADR 0006 §11): the
// orchestration around a `CoseClient`, for the generated REST runtime (a
// `fetch` option) and the generated RPC runtime (an `RpcLink`).
//
// It sits at `fetch` because that is where the exact wire bytes (after the
// codec and after compact keys) and the exact headers (`Idempotency-Key`,
// `If-Match`, the compact `Content-Type` and `Accept`) exist, and it binds
// what the request carries, as the Rust client's `request_sealed` does. It
// holds no crypto and no canonical form: the signature, the hash, the query
// and the AAD are the `CoseClient`'s (Rust). What is left is header plumbing.
import { type CoseCallBinding, type CoseClient, CoseFailure } from "@cratestack/ts-types";

/** The `CoseClient` a call is sealed with, or `undefined` to send it unsealed
 *  (an anonymous call, an op the client does not seal). A function picks per
 *  call, so a Sign1 client can seal the calls before a session and a Mac0
 *  client under the session key the calls after it. */
export type CoseClientSource =
  | CoseClient
  | undefined
  | ((call: { readonly route: string; readonly method: string }) => CoseClient | undefined);

/** One resolved route: what the call is bound to besides its bytes. */
interface SealedRoute {
  readonly route: string;
  readonly pathParams: readonly string[];
  readonly contractSha: Uint8Array;
}

const CBOR = "application/cbor";
const NO_BODY_STATUS = new Set([101, 204, 205, 304]);
const PAYLOAD_TYPE_HEADER = "cratestack-payload-type";

const hexBytes = (hex: string): Uint8Array =>
  Uint8Array.from(hex.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16));

const pick = (source: CoseClientSource, route: string, method: string): CoseClient | undefined =>
  typeof source === "function" ? source({ route, method }) : source;

async function bodyBytes(body: BodyInit | null | undefined): Promise<Uint8Array> {
  if (body === null || body === undefined) return new Uint8Array();
  if (typeof body === "string") return new TextEncoder().encode(body);
  if (body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer) return new Uint8Array(body);
  throw new CoseFailure("misuse", "a sealed call carries a string or bytes body");
}

/** The media type of a header value, parameters dropped. */
const mediaTypeOf = (value: string): string => value.split(";", 1)[0]?.trim().toLowerCase() ?? "";

/** The URL a `fetch` was given, whatever shape it came in. */
const urlOf = (input: RequestInfo | URL): URL =>
  new URL(input instanceof Request ? input.url : input.toString());

/**
 * One sealed round trip: seals `init.body` as the call `route` describes, sends
 * it through `fetchFn`, and opens the answer. Resolves to a `Response` whose
 * body is the opened payload and whose `Content-Type` is the type it was sealed
 * under, so everything downstream of `fetch` is the code an unsealed call runs.
 *
 * - An answer that opens is returned; one that does not rejects with a
 *   {@link CoseFailure} `rejected` (tampered, replayed, wrong audience, wrong
 *   signer, unrequested type), with no detail.
 * - An unsigned answer to a refused call (`!ok`: a gateway's `401`, the
 *   server's `415` or `426`) is returned untouched, a hint and never proof. An
 *   unsigned *success* is never believed: it rejects.
 */
async function sealedFetch(
  fetchFn: typeof fetch,
  cose: CoseClient,
  target: SealedRoute,
  input: RequestInfo | URL,
  init: RequestInit = {},
): Promise<Response> {
  const url = urlOf(input);
  const headers = new Headers(init.headers);
  const payload = await bodyBytes(init.body);
  const payloadType = mediaTypeOf(headers.get("content-type") ?? "") || CBOR;
  const accept = (headers.get("accept") ?? "")
    .split(",")
    .map(mediaTypeOf)
    .filter((type) => type !== "");
  const query = url.search.startsWith("?") ? url.search.slice(1) : url.search;
  const binding: CoseCallBinding = {
    method: (init.method ?? "GET").toUpperCase(),
    route: target.route,
    pathParams: target.pathParams,
    query: query === "" ? undefined : cose.canonicalQuery(query),
    contractSha: target.contractSha,
    idempotencyKey: headers.get("idempotency-key") ?? undefined,
    ifMatch: headers.get("if-match") ?? undefined,
    payloadType,
    payloadAccept: accept.length === 0 ? undefined : accept,
  };
  if (
    target.route === "batch" &&
    (payloadType !== CBOR || (accept.length > 0 && !accept.includes(CBOR)))
  ) {
    throw new CoseFailure("misuse", "a sealed /rpc/batch is CBOR both ways");
  }
  const sealed = await cose.sealRequest(payload, binding);
  headers.set("content-type", cose.mediaType);
  headers.set("accept", cose.mediaType);
  headers.set("cratestack-contract", cose.contractHeader(target.contractSha));
  for (const name of ["cratestack-payload-type", "cratestack-payload-accept"]) headers.delete(name);
  for (const [name, value] of Object.entries(cose.selectorHeaders(binding))) {
    headers.set(name, value);
  }

  const response = await fetchFn(url, { ...init, headers, body: sealed as BodyInit });
  const contentType = mediaTypeOf(response.headers.get("content-type") ?? "");
  if (contentType !== mediaTypeOf(cose.mediaType)) {
    if (response.ok) {
      throw new CoseFailure("rejected", "");
    }
    return response;
  }
  const opened = await cose.openResponse(
    new Uint8Array(await response.arrayBuffer()),
    binding,
    sealed,
    response.status,
    response.headers.get(PAYLOAD_TYPE_HEADER) ?? undefined,
  );
  const answered = new Headers(response.headers);
  for (const name of ["content-length", "content-encoding", PAYLOAD_TYPE_HEADER]) {
    answered.delete(name);
  }
  answered.set("content-type", opened.payloadType);
  return new Response(NO_BODY_STATUS.has(response.status) ? null : (opened.payload as BodyInit), {
    status: response.status,
    statusText: response.statusText,
    headers: answered,
  });
}

/** The `OP_CONTRACTS` of a generated client. */
export type OpContracts = Readonly<Record<string, string>>;

/** The slice of a generated RPC runtime's link types this file needs. The
 *  generated `RpcLink` is structurally this. */
interface RpcSealRequest {
  readonly kind: "unary" | "batch";
  readonly opId: string;
  readonly fetchFn: typeof fetch;
}
type RpcSealLink = <R extends RpcSealRequest, T>(
  request: R,
  next: (request: R) => Promise<T>,
) => Promise<T>;

/**
 * An `RpcLink` that seals every unary and batch call of a generated RPC client:
 * the route is the op id (`batch` for `/rpc/batch`), the contract digest is
 * `OP_CONTRACTS[route]`. An op with no contract, or one `cose` declines
 * (`undefined`), goes out unsealed. `stream()` is not a link of this chain and
 * is never sealed (chain-mode streams are not implemented).
 *
 * ```ts
 * import { createCoseClient, createSealedLink } from "@cratestack/cbor/cose";
 * import { CratestackRpcRuntime, OP_CONTRACTS } from "./runtime";
 *
 * const runtime = new CratestackRpcRuntime(origin, {
 *   links: [createSealedLink({ cose, contracts: OP_CONTRACTS })],
 * });
 * ```
 */
export function createSealedLink(options: {
  cose: CoseClientSource;
  contracts: OpContracts;
}): RpcSealLink {
  return (request, next) => {
    const route = request.kind === "batch" ? "batch" : request.opId;
    const client = pick(options.cose, route, "POST");
    const contract = options.contracts[route];
    if (client === undefined || contract === undefined) {
      return next(request);
    }
    const target: SealedRoute = { route, pathParams: [], contractSha: hexBytes(contract) };
    const sealedFetchFn = ((input: RequestInfo | URL, init?: RequestInit) =>
      sealedFetch(request.fetchFn, client, target, input, init)) as typeof fetch;
    return next({ ...request, fetchFn: sealedFetchFn });
  };
}

interface RestTemplate {
  readonly key: string;
  readonly method: string;
  readonly template: string;
  readonly pattern: RegExp;
  readonly literals: number;
  readonly contractSha: Uint8Array;
}

/** `"<METHOD> <template>"` keys of `OP_CONTRACTS` as matchers over a path. */
function restTemplates(contracts: OpContracts): RestTemplate[] {
  const out: RestTemplate[] = [];
  for (const [key, contract] of Object.entries(contracts)) {
    const space = key.indexOf(" ");
    if (space < 0) continue;
    const template = key.slice(space + 1);
    const segments = template.split("/");
    const pattern = new RegExp(
      `^${segments
        .map((segment) =>
          /^\{[^}]+\}$/.test(segment) ? "([^/]+)" : segment.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"),
        )
        .join("/")}$`,
    );
    out.push({
      key,
      method: key.slice(0, space),
      template,
      pattern,
      literals: segments.filter((segment) => !/^\{[^}]+\}$/.test(segment)).length,
      contractSha: hexBytes(contract),
    });
  }
  // The most literal template wins (`/widgets/search` before `/widgets/{id}`).
  return out.sort((a, b) => b.literals - a.literals);
}

/**
 * A `fetch` that seals every call of a generated REST client, for
 * `CratestackRuntime`'s `fetch` option. The route template and path parameters
 * come from matching the URL path (after `basePath`, the runtime's own) against
 * the `"<METHOD> <template>"` keys of `OP_CONTRACTS`; a call that matches none,
 * or that `cose` declines (`undefined`), goes out unsealed.
 *
 * ```ts
 * const runtime = new CratestackRuntime(origin, {
 *   fetch: createSealedFetch({ cose, contracts: OP_CONTRACTS, basePath: "/api" }),
 * });
 * ```
 */
export function createSealedFetch(options: {
  cose: CoseClientSource;
  contracts: OpContracts;
  basePath?: string;
  fetch?: typeof fetch;
}): typeof fetch {
  const templates = restTemplates(options.contracts);
  const basePath = (options.basePath ?? "").replace(/\/+$/, "");
  const underlying = options.fetch ?? fetch.bind(globalThis);
  return (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = urlOf(input);
    const method = (
      init?.method ?? (input instanceof Request ? input.method : "GET")
    ).toUpperCase();
    const path =
      basePath !== "" && url.pathname.startsWith(basePath)
        ? url.pathname.slice(basePath.length)
        : url.pathname;
    for (const candidate of templates) {
      const match = candidate.method === method ? candidate.pattern.exec(path) : null;
      if (match === null) continue;
      const client = pick(options.cose, candidate.key, method);
      if (client === undefined) break;
      const target: SealedRoute = {
        route: candidate.template,
        pathParams: match.slice(1).map(decodeURIComponent),
        contractSha: candidate.contractSha,
      };
      return sealedFetch(underlying, client, target, input, init);
    }
    return underlying(input, init);
  }) as typeof fetch;
}
