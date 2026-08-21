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

  for (const [path, fields] of Object.entries(query.includeFields ?? {})) {
    if (fields.length > 0) {
      output[`includeFields[${path}]`] = fields.join(",");
    }
  }

  return output;
}