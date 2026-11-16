// A signed subscription open (ADR 0006, cratestack#1378): the open is sealed
// like any call, and the stream that answers carries plain frames, labelled
// `Cratestack-Frames: unsigned`. The generated client's subscription readers
// send the open through the `fetch` this builds; they never see a seal.
//
// RPC only: the open is a `POST` with the input as its CBOR body. A REST open
// would be a `GET` with a body, which `fetch` refuses (cratestack#1407), so a
// REST route is a `misuse` before any `fetch`. Only a subscription the schema
// marks `@frames(unsigned)` can be opened this way; the generated client
// checks the mark, the server checks it again.
//
// The frames are hints, not evidence. A response is accepted as the stream only
// when it carries the marker exactly once and the stream type that was asked
// for; a plain `200` a proxy forged, or one that lost its marker, is refused
// with {@link UnsignedFramesRefused}. Every refusal the server sends is
// sealed, and is opened and returned as the response it is.
import { CoseFailure } from "@cratestack/ts-types";
import type { CoseClientSource, OpContracts } from "./sealed.js";
import { hexBytes, pick } from "./sealed.js";
import { mediaTypeOf, openAnswer, sealRequest } from "./sealed-request.js";
import { SealRequired } from "./sealed-required.js";

const FRAMES_HEADER = "cratestack-frames";
const STREAMS = ["application/cbor-seq", "text/event-stream"];

/** The answer to a signed subscription open was not the unsigned stream that
 *  was asked for: it lacks `Cratestack-Frames: unsigned` (exactly once), or its
 *  `Content-Type` is not the stream type requested. Its body was not read as
 *  frames. A {@link CoseFailure} with code `rejected`. */
export class UnsignedFramesRefused extends CoseFailure {
  constructor() {
    super("rejected", "the answer is not the unsigned stream that was asked for");
    this.name = "UnsignedFramesRefused";
  }
}

/** The subscription op an open is for: the generated client's target. */
export interface SealedOpenTarget {
  readonly opId: string;
  readonly ssePath: string;
}

/** What the generated runtime's `subscriptions.seal` option takes. */
export interface SealedSubscriptions {
  /**
   * A `fetch` that seals the open of `target` and sends it through
   * `underlying`. Throws a {@link CoseFailure} `misuse` for a REST route, and a
   * {@link SealRequired} when `cose` declines the op or it has no contract:
   * a subscription that was meant to be sealed is never opened in plain.
   */
  fetchFor(target: SealedOpenTarget, underlying: typeof fetch): typeof fetch;
}

/**
 * Seals the opens of a generated RPC client's subscriptions.
 *
 * ```ts
 * import { createCoseClient, createSealedSubscriptions } from "@cratestack/cbor/cose";
 * import { CratestackRpcRuntime, OP_CONTRACTS } from "./runtime";
 *
 * const runtime = new CratestackRpcRuntime(origin, {
 *   subscriptions: { seal: createSealedSubscriptions({ cose, contracts: OP_CONTRACTS }) },
 * });
 * ```
 */
export function createSealedSubscriptions(options: {
  cose: CoseClientSource;
  contracts: OpContracts;
}): SealedSubscriptions {
  return {
    fetchFor(target, underlying) {
      if (target.ssePath !== `/rpc/subscribe/${target.opId}`) {
        throw new CoseFailure(
          "misuse",
          `a REST subscription cannot be opened sealed (fetch sends no body on a GET): use transport rpc for "${target.opId}"`,
        );
      }
      const cose = pick(options.cose, target.opId, "POST");
      const contract = options.contracts[target.opId];
      if (cose === undefined || contract === undefined) {
        throw new SealRequired(target.opId, contract === undefined ? "no-contract" : "declined");
      }
      const route = {
        route: `subscribe/${target.opId}`,
        pathParams: [],
        contractSha: hexBytes(contract),
      };
      return (async (input: RequestInfo | URL, init: RequestInit = {}) => {
        const stream = mediaTypeOf(new Headers(init.headers).get("accept") ?? "");
        if (!STREAMS.includes(stream) || (init.method ?? "GET").toUpperCase() !== "POST") {
          throw new CoseFailure(
            "misuse",
            "a signed subscription open is a POST asking for a stream",
          );
        }
        const { url, sealed, headers, binding } = await sealRequest(
          cose,
          route,
          input,
          init,
          stream,
        );
        const response = await underlying(url, { ...init, headers, body: sealed as BodyInit });
        if (
          mediaTypeOf(response.headers.get("content-type") ?? "") === mediaTypeOf(cose.mediaType)
        ) {
          // A sealed answer is a refusal, opened; a sealed success is no stream.
          const answer = await openAnswer(cose, response, binding, sealed);
          if (answer.ok) throw new CoseFailure("rejected", "");
          return answer;
        }
        // An unsigned refusal is a hint, returned for what it is.
        if (!response.ok) return response;
        if (
          response.headers.get(FRAMES_HEADER) !== "unsigned" ||
          mediaTypeOf(response.headers.get("content-type") ?? "") !== stream
        ) {
          await response.body?.cancel().catch(() => undefined);
          throw new UnsignedFramesRefused();
        }
        return response;
      }) as typeof fetch;
    },
  };
}
