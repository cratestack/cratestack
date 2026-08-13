/** Shared filter/sort semantics for both fake in-memory servers
 *  (`fake-rest-server.ts`'s `URLSearchParams`-keyed query,
 *  `fake-rpc-server.ts`'s `RpcListPredicate[]`/`sort: string` body) —
 *  factored out so the two fakes provably implement the *same*
 *  `field__operator`/`-field` DSL rather than two hand-drifted copies of
 *  it. Both REST and RPC really do share this DSL server-side: RPC's
 *  `filters`/`sort` are turned into the identical REST-shaped query
 *  string by `crates/cratestack-axum/src/rpc/synthesize.rs` before
 *  reaching the same list handler. */

type Row = Record<string, unknown>;

const IGNORED_QUERY_KEYS = new Set(["limit", "offset", "sort", "fields", "include"]);

/** Applies a flat AND of `field[__operator]=value` predicates, given as
 *  `(key, value)` pairs — REST's caller passes `URLSearchParams.entries()`
 *  directly; RPC's passes `RpcListPredicate[].map(p => [p.key, p.value])`. */
export function applyFilterPairs(items: Row[], pairs: Iterable<[string, string]>): Row[] {
  const predicates: [string, string][] = [...pairs].filter(([key]) => !IGNORED_QUERY_KEYS.has(key));
  if (predicates.length === 0) return items;
  return items.filter((item) =>
    predicates.every(([key, value]) => {
      const separator = key.indexOf("__");
      const field = separator === -1 ? key : key.slice(0, separator);
      const operator = separator === -1 ? "eq" : key.slice(separator + 2);
      return matches(item[field], operator, value);
    }),
  );
}

function matches(actual: unknown, operator: string, value: string): boolean {
  switch (operator) {
    case "eq":
      return String(actual) === value;
    case "ne":
      return String(actual) !== value;
    case "in":
      return value.split(",").includes(String(actual));
    case "lt":
      return Number(actual) < Number(value);
    case "lte":
      return Number(actual) <= Number(value);
    case "gt":
      return Number(actual) > Number(value);
    case "gte":
      return Number(actual) >= Number(value);
    case "contains":
      return String(actual).includes(value);
    case "startsWith":
      return String(actual).startsWith(value);
    case "isNull":
      return String(actual === null || actual === undefined) === value;
    default:
      return true;
  }
}

/** `field`/`-field`, comma-separated — the one sort DSL both transports'
 *  list handlers parse (`crates/cratestack-macros/src/axum/model/serializers.rs`'s
 *  `sort.split(',')`). REST's caller passes the URL's `?sort=` value; RPC's
 *  passes `CratestackRpcListQuery.sort` directly (already a single string,
 *  not REST's client-side-joined `string[]`). */
export function applySort(items: Row[], sortParam: string | null | undefined): Row[] {
  if (!sortParam) return items;
  const fields = sortParam.split(",");
  return [...items].sort((a, b) => {
    for (const raw of fields) {
      const desc = raw.startsWith("-");
      const field = desc ? raw.slice(1) : raw;
      const left = a[field] as number | string;
      const right = b[field] as number | string;
      if (left < right) return desc ? 1 : -1;
      if (left > right) return desc ? -1 : 1;
    }
    return 0;
  });
}
