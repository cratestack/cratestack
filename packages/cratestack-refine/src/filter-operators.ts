/** refine `CrudFilters` operator -> the generated list route's
 *  `field__<operator>=value` query-string suffix (`null` means "no
 *  suffix", i.e. plain equality). Shared by both transports' list-query
 *  builders — `filters.ts` (REST, `Record<string,string>`) and
 *  `rpc-filters.ts` (RPC, `RpcListPredicate[]`) — because the underlying
 *  `field__op` key convention is identical on the wire: both are
 *  generated from the same per-field arm table
 *  (`crates/cratestack-macros/src/axum/filter_arms.rs::generate_query_filter_arm`),
 *  confirmed by reading `crates/cratestack-axum/src/rpc/synthesize.rs`,
 *  which turns an RPC `filters: [{key, value}]` predicate into the exact
 *  same `key=value` query pair the REST list handler parses. Keeping this
 *  table in ONE place is what makes "REST and RPC agree on filter
 *  operators" a fact rather than something that can drift between two
 *  copies. Also keep this in lockstep with `guides/refine-integration.md`
 *  in cratestack-docs, which documents the identical mapping for
 *  hand-wired consumers. */
const SUPPORTED_OPERATOR_SUFFIX: Record<string, string | null> = {
  eq: null,
  ne: "ne",
  in: "in",
  lt: "lt",
  lte: "lte",
  gt: "gt",
  gte: "gte",
  contains: "contains",
  startswith: "startsWith",
  null: "isNull",
  nnull: "isNull",
};

/** Every operator this map understands, for the error message below. */
const SUPPORTED_OPERATORS = Object.keys(SUPPORTED_OPERATOR_SUFFIX);

/** Returns the `field__<suffix>` query key's suffix (`null` for bare
 *  equality), or **throws** for a refine operator with no cratestack
 *  equivalent (`endswith`, `between`, `nin`, `containss`, …). Throwing
 *  rather than silently dropping the filter matters: a dropped filter
 *  would show the caller an unfiltered result set as if it were
 *  filtered, which is worse than an error because nothing signals the
 *  data is wrong. */
export function requireOperatorSuffix(field: string, operator: string): string | null {
  if (!(operator in SUPPORTED_OPERATOR_SUFFIX)) {
    throw new Error(
      `refine operator "${operator}" on "${field}" has no cratestack equivalent ` +
        `(supported: ${SUPPORTED_OPERATORS.join(", ")}) — failing loudly instead of silently ` +
        "returning unfiltered data",
    );
  }
  return SUPPORTED_OPERATOR_SUFFIX[operator] ?? null;
}

/** Renders a refine filter's `value` into the string every cratestack
 *  query key expects on the wire, for either transport. */
export function filterValue(operator: string, value: unknown): string {
  if (operator === "null") return "true";
  if (operator === "nnull") return "false";
  if (Array.isArray(value)) return value.map(String).join(",");
  if (value instanceof Date) return value.toISOString();
  return String(value);
}
