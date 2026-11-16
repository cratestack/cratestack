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
import { type CoseClient, CoseFailure } from "@cratestack/ts-types";
import { mediaTypeOf, openAnswer, type SealedRoute, sealRequest } from "./sealed-request.js";
import { SealRequired } from "./sealed-required.js";

export type { SealedRoute } from "./sealed-request.js";
export { urlOf } from "./sealed-request.js";
export { SealRequired } from "./sealed-required.js";
export { createSealedFetch } from "./sealed-rest.js";
export type { SealedSubscriptions } from "./sealed-stream.js";
export { createSealedSubscriptions, UnsignedFramesRefused } from "./sealed-stream.js";

/** The `CoseClient` a call is sealed with, or `undefined` to send it unsealed
 *  (an anonymous call, an op the client does not seal). A function picks per
 *  call, so a Sign1 client can seal the calls before a session and a Mac0
 *  client under the session key the calls after it. */
export type CoseClientSource =
  | CoseClient
  | undefined
  | ((call: { readonly route: string; readonly method: string }) => CoseClient | undefined);

export const hexBytes = (hex: string): Uint8Array =>
  Uint8Array.from(hex.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16));

export const pick = (
  source: CoseClientSource,
  route: string,
  method: string,
): CoseClient | undefined => (typeof source === "function" ? source({ route, method }) : source);

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
export async function sealedFetch(
  fetchFn: typeof fetch,
  cose: CoseClient,
  target: SealedRoute,
  input: RequestInfo | URL,
  init: RequestInit = {},
): Promise<Response> {
  const { url, sealed, headers, binding } = await sealRequest(cose, target, input, init);
  const response = await fetchFn(url, { ...init, headers, body: sealed as BodyInit });
  const contentType = mediaTypeOf(response.headers.get("content-type") ?? "");
  if (contentType !== mediaTypeOf(cose.mediaType)) {
    if (response.ok) {
      throw new CoseFailure("rejected", "");
    }
    return response;
  }
  return openAnswer(cose, response, binding, sealed);
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
 * (`undefined`), goes out unsealed unless `requireSealed` is `true`; then the
 * call is refused locally, before any `fetch`, with a {@link SealRequired}
 * naming the route (use it where a silent downgrade to plaintext is worse than
 * a failed call). `requireSealed` defaults to `false`. `stream()` is not a link of this chain and
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
  requireSealed?: boolean;
}): RpcSealLink {
  return (request, next) => {
    const route = request.kind === "batch" ? "batch" : request.opId;
    const client = pick(options.cose, route, "POST");
    const contract = options.contracts[route];
    if (client === undefined || contract === undefined) {
      if (options.requireSealed === true) {
        return Promise.reject(
          new SealRequired(route, contract === undefined ? "no-contract" : "declined"),
        );
      }
      return next(request);
    }
    const target: SealedRoute = { route, pathParams: [], contractSha: hexBytes(contract) };
    const sealedFetchFn = ((input: RequestInfo | URL, init?: RequestInit) =>
      sealedFetch(request.fetchFn, client, target, input, init)) as typeof fetch;
    return next({ ...request, fetchFn: sealedFetchFn });
  };
}
