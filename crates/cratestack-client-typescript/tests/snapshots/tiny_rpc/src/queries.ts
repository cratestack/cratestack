// Typed list-query builder for RPC transport (issue #333). Unlike REST's
// `CratestackFetchQuery` (the sibling `queries.ts` on the REST side), this
// does not build a URL query string — an RPC `list` call's input is a
// plain object handed straight to the runtime's codec (JSON by default,
// CBOR if configured) and POSTed as the request body, so there is no
// query-string layer to shape.
//
// The wire shape below mirrors `cratestack_axum::rpc::RpcListInput` field
// for field, because the server decodes the RPC body directly into that
// struct (`cratestack-macros/src/transport/rpc.rs`'s
// `decode_rpc_body::<_, RpcListInput>`) and turns it into a URL query
// string for the existing REST list handler via `synthesize_list_query`.
// See that struct's own doc comment in
// `crates/cratestack-axum/src/rpc/inputs.rs` for the authoritative
// field-by-field contract.
//
// `where`/`or` are pre-built flat filter-expression strings — `key=value`
// predicates joined by `,` for AND, `|` for OR, `not(...)` for negation,
// parens for grouping (see `crates/cratestack-axum/src/query.rs`'s
// `FilterExpressionParser`) — NOT JSON objects. This is a deliberate
// departure from REST's `CratestackFetchQuery.where` /`.filters`
// /`.orFilters` (typed `Record<string, unknown>` / arrays of it,
// JSON-stringified into the URL by `rest-runtime.ts.j2`'s
// `appendQueryValue`): that shape does not actually match the server's
// filter-expression grammar. A JSON-stringified `where` value has no
// top-level `key=value` and fails `FilterExpressionParser::parse_predicate`
// with a 400 — confirmed by reading `crates/cratestack-axum/src/query.rs`
// directly, not assumed. This file does not carry that mismatch forward;
// `where`/`or` here are plain strings the caller builds in the same DSL
// the server actually parses.
//
// `includeFields` is the one field whose wire key does not follow this
// package's camelCase convention: `RpcListInput` has no
// `#[serde(rename_all = "camelCase")]`, so its wire key is the literal
// Rust field name `include_fields`. `toRpcListInput` below does that one
// translation so callers still get an idiomatic camelCase property.

/** A single arbitrary `(key, value)` predicate — the RPC equivalent of an
 *  unreserved REST query param (e.g. `?published=true`). Mirrors
 *  `cratestack_axum::rpc::RpcListPredicate` exactly. */
export interface RpcListPredicate {
  key: string;
  value: string;
}

/** Typed input for `model.<X>.list` RPC calls — the RPC counterpart of
 *  REST's `CratestackFetchQuery`. Every field maps directly onto an
 *  `RpcListInput` field of the same semantics; see this file's header
 *  comment for the two departures from `CratestackFetchQuery` (`where`/
 *  `or` are DSL strings, not objects; `includeFields` serializes under
 *  the wire key `include_fields`).
 *
 *  `TComputedParams` is this model's own generated `<Model>ComputedParams`
 *  interface (`docs/design/computed-fields.md`) when the model declares a
 *  parameterized `@computed(params: <Type>?)` field, or the default
 *  `never` otherwise — same per-model gate `CratestackFetchQuery` uses on
 *  the REST side, enforced by `tsc`. */
export interface CratestackRpcListQuery<TComputedParams = never> {
  limit?: number;
  offset?: number;
  fields?: string[];
  include?: string[];
  includeFields?: Record<string, string[]>;
  sort?: string;
  where?: string;
  or?: string;
  filters?: RpcListPredicate[];
  /** Params for `@computed(params: <Type>?)` resolver fields, keyed by
   *  computed field name — e.g. `{ proxyUrl: { width: 800 } }`. Unlike
   *  REST's `CratestackFetchQuery.computedParams`, this travels on the
   *  wire as `RpcListInput::computed_params`'s raw JSON-object TEXT (a
   *  `String`, not a nested object) — see that field's own doc comment
   *  in `cratestack-core::rpc::inputs` for why (CBOR `Option::None`
   *  corruption through `serde_json::Value`, `/rpc/batch`'s re-encode
   *  round trip). `toRpcListInput` below does the `JSON.stringify`. */
  computedParams?: TComputedParams;
}

/** Builds the exact object shape `RpcListInput` expects on the wire.
 *  Omits every unset/empty field, mirroring `RpcListInput`'s
 *  `skip_serializing_if` attributes so an empty query serializes the same
 *  as `RpcListInput::default()` — an empty object, `{}`. */
export function toRpcListInput<TComputedParams = never>(
  query?: CratestackRpcListQuery<TComputedParams>,
): Record<string, unknown> {
  const input: Record<string, unknown> = {};
  if (!query) {
    return input;
  }

  if (query.limit !== undefined) {
    input.limit = query.limit;
  }
  if (query.offset !== undefined) {
    input.offset = query.offset;
  }
  if (query.fields?.length) {
    input.fields = query.fields;
  }
  if (query.include?.length) {
    input.include = query.include;
  }
  if (query.includeFields && Object.keys(query.includeFields).length > 0) {
    input.include_fields = query.includeFields;
  }
  if (query.sort !== undefined) {
    input.sort = query.sort;
  }
  if (query.where !== undefined) {
    input.where = query.where;
  }
  if (query.or !== undefined) {
    input.or = query.or;
  }
  if (query.filters?.length) {
    input.filters = query.filters;
  }
  if (query.computedParams !== undefined) {
    input.computedParams = JSON.stringify(query.computedParams);
  }

  return input;
}