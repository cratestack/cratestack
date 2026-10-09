// Shared by the four suites in this directory. One generated client per run,
// over a `fetch` that answers from the URL alone, so the same suite drives the
// REST package (`/api/counters/5`, `/api/$procs/balance`) and the RPC one
// (`/api/rpc/model.Counter.get`, `/api/rpc/procedure.balance`);
// `tests/bigint_query_keys.rs` sets `B8_TRANSPORT` to say which.
import { BigintQueryKeysClient } from "./src/client.js";

export const TRANSPORT: "rest" | "rpc" = process.env.B8_TRANSPORT === "rpc" ? "rpc" : "rest";

/** 2^53, and the first integer a double cannot tell apart from it. */
export const TWO_53 = 2n ** 53n;
export const TWO_53_PLUS_ONE = TWO_53 + 1n;

export const COUNTER = { id: "5", label: "c", hitsE8: "9007199254740993" };
const LEDGER = { id: "l1", reference: "r", amountE8: "9007199254740993" };

export interface Sent {
  url: string;
  method: string;
  text: string;
}

function respond(url: string): unknown {
  if (url.includes("balance")) return "7";
  if (url.includes("searchCounters")) return [COUNTER];
  if (url.includes("bump")) return COUNTER;
  if (url.includes("Ledger.list") || /\/ledgers(\?|$)/.test(url)) {
    return {
      items: [LEDGER],
      totalCount: 1,
      pageInfo: { limit: null, offset: null, hasNextPage: false, hasPreviousPage: false },
    };
  }
  if (url.includes("Counter.list") || /\/counters(\?|$)/.test(url)) return [COUNTER];
  return COUNTER;
}

export const recordingFetch =
  (sent: Sent[]): typeof fetch =>
  async (input, init) => {
    const url = String(input);
    sent.push({
      url,
      method: init?.method ?? "GET",
      text: typeof init?.body === "string" ? init.body : "",
    });
    return new Response(JSON.stringify(respond(url)), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };

export function makeClient(): { client: BigintQueryKeysClient; sent: Sent[] } {
  const sent: Sent[] = [];
  return {
    client: new BigintQueryKeysClient("http://example.invalid", { fetch: recordingFetch(sent) }),
    sent,
  };
}

/** Everything one request carried, URL and body, to search for a value in. */
export const wire = (sent: Sent[]): string => sent.map((s) => `${s.url} ${s.text}`).join("\n");

/** How many requests touched `needle` (a URL fragment or a body fragment). */
export const countRequests = (sent: Sent[], needle: string): number =>
  sent.filter((s) => `${s.url} ${s.text}`.includes(needle)).length;
