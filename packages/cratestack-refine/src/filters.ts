import type { CrudFilters, CrudSorting } from "@refinedev/core";

/** refine `CrudFilters` operator -> the generated list route's
 *  `field__<operator>=value` query-string suffix (`null` means "no
 *  suffix", i.e. plain equality). This is the *same* operator set the
 *  generated TypeScript client's shared filter interfaces expose
 *  (`EqualityFilter<V> { eq, ne, in, isNull }`, `ComparableFilter<V>
 *  extends EqualityFilter<V> { lt, lte, gt, gte }`, `StringFilter
 *  extends ComparableFilter<string> { contains, startsWith }` —
 *  `crates/cratestack-client-typescript/templates/src/models.ts.j2`),
 *  because both are generated from the same per-field arm table
 *  (`crates/cratestack-macros/src/axum/filter_arms.rs::generate_query_filter_arm`).
 *  Keep this table in lockstep with `guides/refine-integration.md` in
 *  cratestack-docs — the two are meant to describe the identical
 *  mapping. */
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

/** Converts refine's `CrudFilters` into the generated list route's flat
 *  `field__operator=value` query-param convention. refine operators with
 *  no cratestack equivalent — `endswith`, `between`, `nin`, `containss`,
 *  and every refine conditional-filter group (`or`/`and`) — **throw**
 *  rather than silently dropping the filter: a dropped filter would show
 *  the caller an unfiltered result set as if it were filtered, which is
 *  worse than an error because nothing signals the data is wrong. */
export function toQueryFilters(filters: CrudFilters = []): Record<string, string> {
  const output: Record<string, string> = {};
  for (const filter of filters) {
    if (!("field" in filter)) {
      throw new Error(
        `refine "${filter.operator}" filter groups have no cratestack equivalent — ` +
          "this dataProvider only supports a flat AND of per-field predicates",
      );
    }
    if (!(filter.operator in SUPPORTED_OPERATOR_SUFFIX)) {
      throw new Error(
        `refine operator "${filter.operator}" on "${filter.field}" has no cratestack equivalent ` +
          `(supported: ${SUPPORTED_OPERATORS.join(", ")}) — failing loudly instead of silently ` +
          "returning unfiltered data",
      );
    }
    const suffix = SUPPORTED_OPERATOR_SUFFIX[filter.operator];
    const key = suffix ? `${filter.field}__${suffix}` : filter.field;
    output[key] = toFilterValue(filter.operator, filter.value);
  }
  return output;
}

function toFilterValue(operator: string, value: unknown): string {
  if (operator === "null") return "true";
  if (operator === "nnull") return "false";
  if (Array.isArray(value)) return value.map(String).join(",");
  if (value instanceof Date) return value.toISOString();
  return String(value);
}

/** refine `CrudSorting` -> the generated list route's `sort` query
 *  values (`field` ascending, `-field` descending). */
export function toSortQuery(sorters: CrudSorting = []): string[] | undefined {
  if (sorters.length === 0) return undefined;
  return sorters.map((s) => (s.order === "desc" ? `-${s.field}` : s.field));
}
