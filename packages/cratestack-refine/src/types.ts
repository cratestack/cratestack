/** The structural surface a generated model class (`client.widgets`,
 *  `client.ledgers`, …) needs for this package to drive it — matched
 *  against the real shape emitted by `cratestack generate-typescript`
 *  (`crates/cratestack-client-typescript/templates/src/client.ts.j2`),
 *  not a hand-invented interface. Each generated model class is its own
 *  concrete TypeScript class with no shared base type the generator
 *  exports, so this is duck-typed rather than an `implements` target —
 *  every real generated class satisfies it structurally. */
export interface CratestackModelApi<
  TModel = unknown,
  TCreateInput = unknown,
  TUpdateInput = unknown,
> {
  list(options?: {
    query?: CratestackFetchQuery;
    headers?: HeadersInit;
    signal?: AbortSignal;
  }): Promise<TModel[] | CratestackPage<TModel>>;
  get(
    id: unknown,
    options?: { query?: CratestackFetchQuery; headers?: HeadersInit; signal?: AbortSignal },
  ): Promise<TModel>;
  /** Absent on the generated class entirely when the model declares no
   *  `@@allow("create", ...)` policy at all — see the package README's
   *  "Policy-denied operations" note for why that's a different, weaker
   *  guarantee than "the current caller can create one". */
  create?(input: TCreateInput, options?: CratestackRequestConfig): Promise<TModel>;
  update(id: unknown, input: TUpdateInput, options?: CratestackRequestConfig): Promise<TModel>;
  delete(id: unknown, options?: CratestackRequestConfig): Promise<void>;
}

/** Mirrors the generated client's own `CratestackFetchQuery`
 *  (`queries.ts.j2`) structurally — only the subset this package writes
 *  to (`filters`, `sort`, `limit`, `offset`). */
export interface CratestackFetchQuery {
  filters?: Record<string, string>;
  sort?: string[];
  limit?: number;
  offset?: number;
}

export interface CratestackRequestConfig {
  signal?: AbortSignal;
  headers?: HeadersInit;
}

/** Mirrors `cratestack_core::page::Page<T>` (`models.ts.j2`) — only the
 *  `items`/`totalCount` fields this package reads. */
export interface CratestackPage<T> {
  items: T[];
  totalCount: number | null;
}

/** Structural match for the generated client's `CratestackHttpError`
 *  (`rest-runtime.ts.j2`) — checked with {@link isCratestackHttpError}
 *  rather than `instanceof`, since that class has no shared import path
 *  this package can reference (it's regenerated per consumer, into
 *  their own package). */
export interface CratestackHttpErrorLike {
  status: number;
  response?: unknown;
  payload?: unknown;
  message?: string;
}

/** One resource's binding to a generated model class. Every field here
 *  is a fact about the `.cstack` model that the generated client's own
 *  TypeScript types encode at compile time but expose nowhere at
 *  runtime — there is no `client.widgets.$meta` object to introspect,
 *  confirmed by reading the generated `client.ts`/`models.ts` output.
 *
 *  Hand-write it, or let `cratestack generate-typescript --refine` emit
 *  it from the schema those facts came from — see the README's "A
 *  runtime package with a generated manifest". */
export interface ResourceConfig<TModel = unknown, TCreateInput = unknown, TUpdateInput = unknown> {
  api: CratestackModelApi<TModel, TCreateInput, TUpdateInput>;
  /** The schema's `@id` field name. refine assumes `id`; cratestack's
   *  `@id` may be on any field (`crates/cratestack-client-typescript/src/types.rs::primary_key_field`). */
  primaryKey: string;
  /** Mirrors whether the model declares `@@paged` — gates whether
   *  `.list()` returns `Page<TModel>` (with a real `totalCount`) or a
   *  bare `TModel[]`. A non-`@@paged` resource cannot report a true
   *  `total` beyond what one page returned — see the package README's
   *  "Pagination" section for why, and set `false` here honestly rather
   *  than treating a non-paged model as paged. */
  paged: boolean;
  /** Mirrors whether the model declares `@version`, and which field
   *  it's on. Omit for a model with no `@version` field — `update`/
   *  `deleteOne` then send no `If-Match` at all, matching the server's
   *  own behavior of not requiring one. */
  versionField?: string;
}

// A resource map is heterogeneous by nature — each resource's own
// TModel/TCreateInput/TUpdateInput differ, and createCratestackDataProvider
// itself is generic over none of them (it returns one DataProvider spanning
// every configured resource). `unknown` would forbid passing a real,
// concretely-typed CratestackModelApi<Widget, ...> into this map.
// biome-ignore lint/suspicious/noExplicitAny: see comment above.
export type ResourceMap = Record<string, ResourceConfig<any, any, any>>;
