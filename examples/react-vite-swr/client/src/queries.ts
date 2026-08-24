export interface CratestackFetchQuery {
  fields?: string[];
  include?: string[];
  includeFields?: Record<string, string[]>;
  sort?: string[];
  limit?: number;
  offset?: number;
  /** Top-level filter expression in the server's `?where=` DSL, e.g. `"published=true,authorId=42"`. */
  where?: string;
  /** Disjunction filter in the server's `?or=` DSL, e.g. `"role=admin|role=owner"`. */
  or?: string;
  /** Arbitrary `key=value` predicates spread as individual query params, e.g. `{ published: "true" }` → `?published=true`. */
  filters?: Record<string, string>;
  /**
   * Params for `@computed(params: <Type>?)` resolver fields
   * (`docs/design/computed-fields.md`), keyed by computed field name —
   * e.g. `{ proxyUrl: { width: 800 } }`. Serialized as a single JSON-
   * encoded `computedParams` query parameter
   * (`appendQueryValue`'s object branch in `runtime.ts` already
   * `JSON.stringify`s any object-valued query entry, so no extra
   * encoding step is needed here). Untyped (`Record<string, unknown>`)
   * rather than a generated per-model params type — v1 escape hatch,
   * see `docs/design/computed-fields.md`'s "Downstream" section. Applies
   * to `get`/`list` only, same as the server's own `?computedParams=`
   * support (root model, read paths only).
   */
  computedParams?: Record<string, unknown>;
}

export interface CratestackRequestConfig {
  signal?: AbortSignal;
  headers?: HeadersInit;
}

export interface CratestackQueryRequestConfig extends CratestackRequestConfig {
  query?: CratestackFetchQuery;
}

// Issue #610: the WRITE half of the ETag/If-Match round trip — an
// optional `ifMatch` argument on generated `update`/`delete` methods,
// mirroring the Rust server-side query builder's own `.if_match(version)`.
// The generated server requires `If-Match` on PATCH (and, since
// cratestack#519, DELETE) for any model with an `@version` field, and
// rejects a stale or missing value with `412 Precondition Failed`. Read
// the matching value off a prior read's `ETag` — e.g.
// `(await client.<model>.getWithResponse(id)).response.headers.get("etag")`.
export interface CratestackWriteRequestConfig extends CratestackRequestConfig {
  ifMatch?: string;
}

// Merges `ifMatch` into `headers` as the `If-Match` request header when
// present, otherwise returns `headers` unchanged — `undefined` in,
// `undefined` out, so callers who never pass `ifMatch` see no behavior
// change and `CratestackRequestOptions.headers` stays satisfied under
// `exactOptionalPropertyTypes`.
export function withIfMatchHeader(
  headers: HeadersInit | undefined,
  ifMatch: string | undefined,
): HeadersInit | undefined {
  if (ifMatch === undefined) {
    return headers;
  }
  const merged = new Headers(headers);
  merged.set("If-Match", ifMatch);
  return merged;
}

export function toSearchQuery(query?: CratestackFetchQuery): Record<string, unknown> | undefined {
  if (!query) {
    return undefined;
  }

  const output: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(query.filters ?? {})) {
    output[key] = value;
  }
  if (query.fields?.length) {
    output.fields = query.fields.join(",");
  }
  if (query.include?.length) {
    output.include = query.include.join(",");
  }
  if (query.sort?.length) {
    output.sort = query.sort.join(",");
  }
  if (query.limit !== undefined) {
    output.limit = query.limit;
  }
  if (query.offset !== undefined) {
    output.offset = query.offset;
  }
  if (query.where) {
    output.where = query.where;
  }
  if (query.or) {
    output.or = query.or;
  }
  if (query.computedParams && Object.keys(query.computedParams).length > 0) {
    output.computedParams = query.computedParams;
  }

  for (const [path, fields] of Object.entries(query.includeFields ?? {})) {
    if (fields.length > 0) {
      output[`includeFields[${path}]`] = fields.join(",");
    }
  }

  return output;
}