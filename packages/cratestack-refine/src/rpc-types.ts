import type { CratestackPage } from "./types.js";

/** Mirrors the generated RPC client's own `RpcListPredicate`
 *  (`crates/cratestack-client-typescript/templates/src/rpc-queries.ts.j2`)
 *  exactly — a single arbitrary `(key, value)` predicate, where `key`
 *  already carries the `field[__operator]` suffix (`toRpcQueryFilters` in
 *  `rpc-filters.ts` builds these). */
export interface RpcListPredicate {
  key: string;
  value: string;
}

/** Mirrors the generated RPC client's own `CratestackRpcListQuery`
 *  structurally — only the subset this package writes to (`filters`,
 *  `sort`, `limit`, `offset`). Unlike REST's `CratestackFetchQuery`,
 *  `sort` is a single already-joined string and `filters` is an array of
 *  {@link RpcListPredicate} rather than a `Record<string,string>` — see
 *  `rpc-filters.ts`'s doc comments for why each differs from its REST
 *  counterpart. `where`/`or`/`fields`/`include`/`includeFields` exist on
 *  the wire but this package never writes to them. */
export interface CratestackRpcListQuery {
  limit?: number;
  offset?: number;
  fields?: string[];
  include?: string[];
  includeFields?: Record<string, string[]>;
  sort?: string;
  where?: string;
  or?: string;
  filters?: RpcListPredicate[];
}

/** Mirrors the generated RPC client's own `CratestackRpcCallOptions`
 *  (`rpc-runtime.ts.j2`) structurally — REST's `CratestackRequestConfig`
 *  sibling, plus `idempotencyKey` (RPC-only; propagated as the
 *  `Idempotency-Key` header). `headers` is where `If-Match` travels for
 *  `update`/`deleteOne` on a `@version` model — the RPC dispatch arms
 *  (`crates/cratestack-macros/src/transport/rpc.rs`) pass the real HTTP
 *  `HeaderMap` straight through to the same `handle_update_*_dispatch`/
 *  `handle_delete_*_dispatch` fns REST uses
 *  (`crates/cratestack-macros/src/axum/model/handlers_update.rs`), which
 *  read it via `parse_if_match_version` — so `If-Match` is enforced
 *  identically on both transports. */
export interface CratestackRpcCallOptions {
  headers?: HeadersInit;
  signal?: AbortSignal;
  idempotencyKey?: string;
}

/** The structural surface a generated RPC model class (`client.widgets`,
 *  `client.ledgers`, …) needs for this package to drive it — matched
 *  against the real shape emitted by `cratestack generate-typescript`
 *  onto a `transport rpc` schema
 *  (`crates/cratestack-client-typescript/templates/src/rpc-client.ts.j2`).
 *  `get`/`create`/`update`/`delete` are positionally identical to REST's
 *  `CratestackModelApi` — only `list` differs, taking the query
 *  positionally (`list(query, options)`) instead of nested in an options
 *  object (REST's `list({ query, headers, signal })`). */
export interface CratestackRpcModelApi<
  TModel = unknown,
  TCreateInput = unknown,
  TUpdateInput = unknown,
> {
  list(
    query?: CratestackRpcListQuery,
    options?: CratestackRpcCallOptions,
  ): Promise<TModel[] | CratestackPage<TModel>>;
  get(id: unknown, options?: CratestackRpcCallOptions): Promise<TModel>;
  /** Absent on the generated class entirely when the model declares no
   *  `@@allow("create", ...)` policy at all — same caveat as REST's
   *  `CratestackModelApi.create`, see the package README's
   *  "Policy-denied operations" note. */
  create?(input: TCreateInput, options?: CratestackRpcCallOptions): Promise<TModel>;
  update(id: unknown, patch: TUpdateInput, options?: CratestackRpcCallOptions): Promise<TModel>;
  delete(id: unknown, options?: CratestackRpcCallOptions): Promise<void>;
}

/** One RPC resource's binding to a generated model class — the RPC
 *  sibling of REST's `ResourceConfig` in `types.ts`, same four facts
 *  (`api`, `primaryKey`, `paged`, optional `versionField`), same
 *  reasoning for why they can't be discovered at runtime and have to be
 *  supplied. Hand-write it (as every example in the README does), or
 *  generate it once `cratestack generate-typescript --refine` grows RPC
 *  support (tracked in cratestack#571's own follow-ups) — as of this
 *  package, the generator still rejects `--refine` on an RPC schema
 *  (`TypeScriptGeneratorError::RefineRequiresRest`), so there is no
 *  generated RPC manifest to assign into this type yet; this package's
 *  own tests build one by hand against a real generated RPC client
 *  instead. */
export interface RpcResourceConfig<
  TModel = unknown,
  TCreateInput = unknown,
  TUpdateInput = unknown,
> {
  api: CratestackRpcModelApi<TModel, TCreateInput, TUpdateInput>;
  /** The schema's `@id` field name. refine assumes `id`; cratestack's
   *  `@id` may be on any field. */
  primaryKey: string;
  /** Mirrors whether the model declares `@@paged` — gates whether
   *  `.list()` returns `Page<TModel>` (with a real `totalCount`) or a
   *  bare `TModel[]`. */
  paged: boolean;
  /** Mirrors whether the model declares `@version`, and which field it's
   *  on. Omit for a model with no `@version` field — `update`/
   *  `deleteOne` then send no `If-Match` at all, matching the server's
   *  own behavior of not requiring one. */
  versionField?: string;
}

// A resource map is heterogeneous by nature — see `ResourceMap`'s
// identical comment in types.ts for why `any` is the honest type here.
// biome-ignore lint/suspicious/noExplicitAny: see comment above.
export type RpcResourceMap = Record<string, RpcResourceConfig<any, any, any>>;
