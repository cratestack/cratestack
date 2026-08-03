// Types referenced by more than one model, or by no model at all (a
// declared-but-unused type — the default preset still emits every
// declared type regardless of use, and this preset preserves that), live
// here and are imported by their consumers. A type referenced by exactly
// one model is instead defined inline in that model's own file, to keep
// single-consumer types co-located with their only consumer. See
// `cratestack-client-typescript`'s `src/swr/ownership.rs::compute_type_ownership`
// for the computation that decides this — this file is its "Shared"
// output, not a hand-maintained list.

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

