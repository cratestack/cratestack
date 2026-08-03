import type { JsonValue } from "./runtime.js";

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
export type DecimalFilter = ComparableFilter<string>;

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

