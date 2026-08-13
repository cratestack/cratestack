import type { CrudFilters, CrudSorting } from "@refinedev/core";
import { filterValue, requireOperatorSuffix } from "./filter-operators.js";

/** Converts refine's `CrudFilters` into the generated REST list route's
 *  flat `field__operator=value` query-param convention (`RpcResourceMap`'s
 *  sibling `toRpcQueryFilters` in `rpc-filters.ts` does the same mapping
 *  for the RPC transport's `RpcListPredicate[]` shape — see
 *  `filter-operators.ts` for the shared operator table both draw from).
 *  refine operators with no cratestack equivalent — `endswith`,
 *  `between`, `nin`, `containss`, and every refine conditional-filter
 *  group (`or`/`and`) — **throw** rather than silently dropping the
 *  filter: a dropped filter would show the caller an unfiltered result
 *  set as if it were filtered, which is worse than an error because
 *  nothing signals the data is wrong. */
export function toQueryFilters(filters: CrudFilters = []): Record<string, string> {
  const output: Record<string, string> = {};
  for (const filter of filters) {
    if (!("field" in filter)) {
      throw new Error(
        `refine "${filter.operator}" filter groups have no cratestack equivalent — ` +
          "this dataProvider only supports a flat AND of per-field predicates",
      );
    }
    const suffix = requireOperatorSuffix(filter.field, filter.operator);
    const key = suffix ? `${filter.field}__${suffix}` : filter.field;
    output[key] = filterValue(filter.operator, filter.value);
  }
  return output;
}

/** refine `CrudSorting` -> the generated list route's `sort` query
 *  values (`field` ascending, `-field` descending). */
export function toSortQuery(sorters: CrudSorting = []): string[] | undefined {
  if (sorters.length === 0) return undefined;
  return sorters.map((s) => (s.order === "desc" ? `-${s.field}` : s.field));
}
