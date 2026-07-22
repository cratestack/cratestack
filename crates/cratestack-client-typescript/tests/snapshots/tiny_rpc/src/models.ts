import type { JsonValue } from "./runtime";

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

export interface EchoNameArgs {
  name: string;
}

