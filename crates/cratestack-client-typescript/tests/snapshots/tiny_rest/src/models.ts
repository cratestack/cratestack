import type { JsonValue } from "./runtime.js";
import DecimalJs from "decimal.js";

// cratestack#498: every `Decimal`-typed schema field is carried as a real
// arbitrary-precision value, not a wire-format-dependent opaque `string`.
// #495/#496 made `decimal-bigdecimal` a real server-side backend for
// values beyond `rust_decimal`'s ~28-29 significant-digit cap, but
// `rust_decimal`'s `Display` never emits scientific notation while
// `bigdecimal`'s does past a magnitude threshold (`"0.0000001"` vs.
// `"1E-7"` for the identical value) — a client that just typed the field
// `string` saw a backend-dependent format and had no way to do
// arithmetic/comparison on it at all. `decimal.js` parses both notations
// into the identical value (`new Decimal("1E-7").equals(new
// Decimal("0.0000001"))` is `true`), closing that gap.
//
// Cloned with an effectively-unbounded exponential-notation threshold
// (`toExpNeg`/`toExpPos`) rather than used directly: decimal.js's own
// default `.toString()`/`.toJSON()` (the encode path every `JSON.stringify`
// call and this package's default JSON-based RPC codec go through — see
// `rpc-runtime.ts.j2`'s `jsonRpcCodec`) switches to scientific notation
// past ±7/21 orders of magnitude, which would make even an *ordinary*
// value like `0.00000001` re-encode as `"1e-8"`. Forcing plain positional
// notation instead matches `rust_decimal`'s own `Display` exactly, so a
// re-encoded value is always accepted back by a server on *either*
// backend without needing to know whether it parses scientific notation.
export const Decimal = DecimalJs.clone({ toExpNeg: -1e9, toExpPos: 1e9 });
export type Decimal = DecimalJs;

/** Recursively walks a decoded response value (REST JSON or this
 *  package's RPC codec output — both produce the same plain
 *  array/object/primitive tree; see `rpc-runtime.ts.j2`'s `jsonRpcCodec`)
 *  and replaces every string value at a key named in `decimalKeys` with a
 *  real {@link Decimal}. Keyed by field *name*, not structural path —
 *  correct for the scope this is actually generated for (cratestack#498
 *  v1): a target type's own *direct* fields, which by definition can't
 *  have two fields with the same name and different types, so there's no
 *  collision to worry about within one model. Does **not** descend into
 *  a relation-embedded field's *own* model's `Decimal` fields (a
 *  `Post.author.balance`-shaped case) — that model's field names would
 *  need to be folded into `decimalKeys` too, which the generator doesn't
 *  do yet (see `views.rs::build_model_api`'s doc comment).
 *
 *  An empty `decimalKeys` is a fast-path identity no-op, so every
 *  generated model API method calls this unconditionally — a model with
 *  no `Decimal` field just passes an empty array literal (see
 *  `views.rs::build_model_api`'s `decimal_fields_js`) — rather than the
 *  generator branching per model in the template. */
export function reviveDecimalFields(value: unknown, decimalKeys: readonly string[]): unknown {
  if (decimalKeys.length === 0) {
    return value;
  }
  return reviveDecimalFieldsInner(value, new Set(decimalKeys));
}

function reviveDecimalFieldsInner(value: unknown, decimalKeys: ReadonlySet<string>): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => reviveDecimalFieldsInner(item, decimalKeys));
  }
  if (value !== null && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      result[key] =
        decimalKeys.has(key) && typeof entry === "string"
          ? new Decimal(entry)
          : reviveDecimalFieldsInner(entry, decimalKeys);
    }
    return result;
  }
  return value;
}

// Mirrors cratestack-core::page::{Page, PageInfo} exactly — this is
// the literal wire shape every `@@paged` list route serializes with
// `#[serde(rename_all = "camelCase")]`, not an independently designed
// client-side type. Keep field names and optionality in lockstep with
// that struct; do not add/rename fields here without changing it
// there first.
export interface PageInfo {
  limit: number | null;
  offset: number | null;
  hasNextPage: boolean;
  hasPreviousPage: boolean;
}

export interface Page<T> {
  items: T[];
  totalCount: number | null;
  pageInfo: PageInfo;
}

// Mirrors cratestack-core::page::PageInput exactly — the request-side
// counterpart to Page/PageInfo above, currently usable only as a
// procedure argument type. Keep field names and optionality in lockstep
// with that struct.
export interface PageInput {
  limit: number | null;
  offset: number | null;
}

// Shared building blocks for every `<Model>Where`/`<Model>FindMany`
// pair below (search-with-filters for procedures — mirrors
// cratestack-core::find_many::FieldFilterInput and
// cratestack-macros's per-model `<Model>Where`/`<Model>SortField`/
// `<Model>OrderByClause`/`<Model>FindManyInput` exactly). Usable only
// as a procedure argument type.
export interface EqualityFilter<V> {
  eq?: V;
  ne?: V;
  in?: V[];
  isNull?: boolean;
}

export interface ComparableFilter<V> extends EqualityFilter<V> {
  lt?: V;
  lte?: V;
  gt?: V;
  gte?: V;
}

export interface StringFilter extends ComparableFilter<string> {
  contains?: string;
  startsWith?: string;
}

export type NumberFilter = ComparableFilter<number>;
export type BooleanFilter = EqualityFilter<boolean>;
export type UuidFilter = ComparableFilter<string>;
export type DateTimeFilter = ComparableFilter<string>;
// cratestack#498: `Decimal`, not `string` — see this file's own
// `Decimal`/`reviveDecimalFields` doc comments above for why. Unlike a
// response's own `Decimal` fields, a `DecimalFilter` only ever travels
// *outbound* as part of a `<Model>Where`/`FindMany` procedure argument
// (`find_many_views.rs`'s own doc comment — "usable only as a procedure
// argument type"), so it needs no `reviveDecimalFields` counterpart on
// decode: `Decimal.prototype.toJSON` (an alias for `.toString()`) makes
// `JSON.stringify` — both a plain REST request body and this package's
// default `jsonRpcCodec` go through it — encode a `Decimal` correctly
// with no generated glue at all.
export type DecimalFilter = ComparableFilter<Decimal>;

export type SortDirection = "asc" | "desc";

export type WidgetSortField = 'id' | 'name' | 'weight';
export const WidgetSortFieldValues = [
  "id",
  "name",
  "weight",
] as const satisfies readonly WidgetSortField[];

export interface Widget {
  id?: number;
  name?: string;
  weight?: number | null;
}

export interface CreateWidgetInput {
  id: number;
  name: string;
  weight?: number | null;
}

export interface UpdateWidgetInput {
  name?: string;
  weight?: number | null;
}

export interface WidgetWhere {
  id?: NumberFilter;
  name?: StringFilter;
  weight?: NumberFilter;
}

export interface WidgetOrderByClause {
  field: WidgetSortField;
  direction: SortDirection;
}

export interface WidgetFindMany {
  where?: WidgetWhere;
  orderBy?: WidgetOrderByClause[];
}

export interface EchoNameArgs {
  name: string;
}

