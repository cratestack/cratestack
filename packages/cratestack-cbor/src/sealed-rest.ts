// The REST half of the fetch-boundary sealing (issue #1010): `createSealedFetch`
// matches a URL to an `OP_CONTRACTS` template and seals through `sealedFetch`.
import { CoseFailure } from "@cratestack/ts-types";
import type { CoseClientSource, OpContracts, SealedRoute } from "./sealed.js";
import { hexBytes, pick, sealedFetch, urlOf } from "./sealed.js";
import { SealRequired } from "./sealed-required.js";

/** Whether the call's `Accept` names a stream type: `sealedFetch` binds the
 *  payload types an answer may be sealed under, and a stream is none. */
function asksForStream(input: RequestInfo | URL, init: RequestInit | undefined): boolean {
  const accept = new Headers(
    init?.headers ?? (input instanceof Request ? input.headers : undefined),
  );
  return (accept.get("accept") ?? "")
    .split(",")
    .some((type) =>
      ["application/cbor-seq", "text/event-stream"].includes(
        type.split(";", 1)[0]?.trim().toLowerCase() ?? "",
      ),
    );
}

function trimTrailingSlashes(value: string): string {
  let end = value.length;
  while (end > 0 && value.charCodeAt(end - 1) === 47) {
    end--;
  }
  return value.slice(0, end);
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
 * or that `cose` declines (`undefined`), goes out unsealed unless
 * `requireSealed` is `true`; then it rejects with a {@link SealRequired} before
 * any `fetch` (default `false`).
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
  requireSealed?: boolean;
}): typeof fetch {
  const templates = restTemplates(options.contracts);
  const basePath = trimTrailingSlashes(options.basePath ?? "");
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
      if (asksForStream(input, init)) {
        throw new CoseFailure(
          "misuse",
          `a stream cannot be sealed: "${candidate.key}" asked for one (a signed subscription open is transport rpc only)`,
        );
      }
      const client = pick(options.cose, candidate.key, method);
      if (client === undefined) {
        if (options.requireSealed === true) throw new SealRequired(candidate.key, "declined");
        break;
      }
      const target: SealedRoute = {
        route: candidate.template,
        pathParams: match.slice(1).map(decodeURIComponent),
        contractSha: candidate.contractSha,
      };
      return sealedFetch(underlying, client, target, input, init);
    }
    if (options.requireSealed === true) throw new SealRequired(`${method} ${path}`, "no-contract");
    return underlying(input, init);
  }) as typeof fetch;
}
