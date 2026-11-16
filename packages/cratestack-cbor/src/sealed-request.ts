// The seal half of a fetch-boundary call (issue #1010): from the request a
// `fetch` was given to the COSE message that replaces its body and the
// headers that go with it. `sealedFetch` (a unary call) and
// `createSealedSubscriptions` (a subscription open, cratestack#1378) both
// seal through it; what they do with the answer differs.
import { type CoseCallBinding, type CoseClient, CoseFailure } from "@cratestack/ts-types";

const NO_BODY_STATUS = new Set([101, 204, 205, 304]);
const PAYLOAD_TYPE_HEADER = "cratestack-payload-type";

/** One resolved route: what the call is bound to besides its bytes. */
export interface SealedRoute {
  readonly route: string;
  readonly pathParams: readonly string[];
  readonly contractSha: Uint8Array;
}

/** A request, sealed. */
export interface SealedRequest {
  readonly url: URL;
  /** The COSE message: the new body. */
  readonly sealed: Uint8Array;
  /** The request's headers with the seal's own set over them. */
  readonly headers: Headers;
  /** What the answer is opened against. */
  readonly binding: CoseCallBinding;
}

export const CBOR = "application/cbor";

/** The media type of a header value, parameters dropped. */
export const mediaTypeOf = (value: string): string =>
  value.split(";", 1)[0]?.trim().toLowerCase() ?? "";

/** The URL a `fetch` was given, whatever shape it came in. */
export const urlOf = (input: RequestInfo | URL): URL =>
  new URL(input instanceof Request ? input.url : input.toString());

async function bodyBytes(body: BodyInit | null | undefined): Promise<Uint8Array> {
  if (body === null || body === undefined) return new Uint8Array();
  if (typeof body === "string") return new TextEncoder().encode(body);
  if (body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer) return new Uint8Array(body);
  throw new CoseFailure("misuse", "a sealed call carries a string or bytes body");
}

/**
 * Seals the call `init` describes as `target`. The call's `Accept` is the
 * payload types it reads back, bound in the seal, unless `stream` names the
 * unsigned stream an open asks for: then `Accept` is that type beside the
 * seal's own (the stream if the server accepts the open, a sealed refusal if
 * it does not) and the payload accept is the CBOR default.
 */
export async function sealRequest(
  cose: CoseClient,
  target: SealedRoute,
  input: RequestInfo | URL,
  init: RequestInit,
  stream?: string,
): Promise<SealedRequest> {
  const url = urlOf(input);
  const headers = new Headers(init.headers);
  const payload = await bodyBytes(init.body);
  const payloadType = mediaTypeOf(headers.get("content-type") ?? "") || CBOR;
  const accept =
    stream !== undefined
      ? []
      : (headers.get("accept") ?? "")
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
  headers.set("accept", stream === undefined ? cose.mediaType : `${stream}, ${cose.mediaType}`);
  headers.set("cratestack-contract", cose.contractHeader(target.contractSha));
  for (const name of ["cratestack-payload-type", "cratestack-payload-accept"]) headers.delete(name);
  for (const [name, value] of Object.entries(cose.selectorHeaders(binding))) {
    headers.set(name, value);
  }
  return { url, sealed, headers, binding };
}

/**
 * Opens the sealed `response` to `sealed` against `binding`: a `Response`
 * whose body is the opened payload and whose `Content-Type` is the type it was
 * sealed under. Rejects with a {@link CoseFailure} `rejected` when it does not
 * open.
 */
export async function openAnswer(
  cose: CoseClient,
  response: Response,
  binding: CoseCallBinding,
  sealed: Uint8Array,
): Promise<Response> {
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
