import type { CrudFilters, CrudSorting } from "@refinedev/core";
import { filterValue, requireOperatorSuffix } from "./filter-operators.js";
import type { RpcListPredicate } from "./rpc-types.js";

/** Converts refine's `CrudFilters` into `RpcListPredicate[]` — the RPC
 *  list route's typed equivalent of REST's `field__operator=value` query
 *  params (`toQueryFilters` in `filters.ts`). Same operator table, same
 *  `field[__suffix]` key on each predicate's `key`, same throw-don't-drop
 *  behavior for an unsupported operator or a refine filter group — see
 *  `filter-operators.ts` for why the two transports share the mapping
 *  rather than each defining their own, and
 *  `crates/cratestack-axum/src/rpc/synthesize.rs` for the server-side
 *  proof that an `RpcListPredicate.key` is spread onto the same
 *  `field__op=value` query string REST's `field__operator` convention
 *  parses. */
export function toRpcQueryFilters(filters: CrudFilters = []): RpcListPredicate[] {
  const output: RpcListPredicate[] = [];
  for (const filter of filters) {
    if (!("field" in filter)) {
      throw new Error(
        `refine "${filter.operator}" filter groups have no cratestack equivalent — ` +
          "this dataProvider only supports a flat AND of per-field predicates",
      );
    }
    const suffix = requireOperatorSuffix(filter.field, filter.operator);
    const key = suffix ? `${filter.field}__${suffix}` : filter.field;
    output.push({ key, value: filterValue(filter.operator, filter.value) });
  }
  return output;
}

/** refine `CrudSorting` -> the RPC list route's `sort` value: the same
 *  `field`/`-field` comma-joined DSL as REST's `toSortQuery` (confirmed
 *  against `crates/cratestack-macros/src/axum/model/serializers.rs`'s
 *  `sort.split(',')` parsing, shared by both transports since RPC
 *  synthesizes a REST-shaped query string server-side), sent as a single
 *  already-joined string instead of REST's `string[]` — the one place
 *  `CratestackRpcListQuery` and `CratestackFetchQuery` genuinely diverge
 *  in shape (`RpcListInput.sort: Option<String>` vs. an array joined
 *  client-side by REST's own runtime). */
export function toRpcSortQuery(sorters: CrudSorting = []): string | undefined {
  if (sorters.length === 0) return undefined;
  return sorters.map((s) => (s.order === "desc" ? `-${s.field}` : s.field)).join(",");
}
