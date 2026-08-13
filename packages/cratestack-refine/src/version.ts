import type { ResourceConfig } from "./types.js";

/** `If-Match` is required on both `PATCH` (update) and `DELETE` for a
 *  `@version` model (cratestack#493/#519, closed by #538 — the
 *  delete-side enforcement deliberately mirrors the update path
 *  token-for-token, `crates/cratestack-macros/src/axum/model/prep/etag.rs`).
 *  refine's `update`/`deleteOne` hooks fetch the record before editing it
 *  (`useOne`/`useShow` populate the edit form; a list/detail view is
 *  usually on screen before a delete button is clicked), so the version
 *  is available by the time a mutation fires — it just isn't part of
 *  refine's `UpdateParams`/`DeleteOneParams` by default. This cache,
 *  populated by every read and write that returns a fresh record, is how
 *  the provider threads it through. One cache per `createCratestackDataProvider`
 *  call, not module-global, so two providers (e.g. two tenants in the
 *  same process) never cross-pollinate versions. */
export function createVersionCache(): Map<string, number> {
  return new Map();
}

function versionKey(resource: string, id: unknown): string {
  return `${resource}:${String(id)}`;
}

export function rememberVersion(
  cache: Map<string, number>,
  resource: string,
  id: unknown,
  record: Record<string, unknown>,
  config: ResourceConfig,
): void {
  if (!config.versionField) return;
  const version = record[config.versionField];
  if (typeof version === "number") {
    cache.set(versionKey(resource, id), version);
  }
}

export function forgetVersion(cache: Map<string, number>, resource: string, id: unknown): void {
  cache.delete(versionKey(resource, id));
}

/** Builds the `If-Match` header for a `@version` model's update/delete,
 *  or `{}` for a model with no `versionField` configured (matching the
 *  server's own behavior of not requiring one). Throws rather than
 *  silently omitting `If-Match` when a version is required but unknown
 *  — sending an update/delete with no `If-Match` against a `@version`
 *  model doesn't fail loudly on the wire (cratestack#519/#538 gate on
 *  the header's *presence*, so a caller can accidentally opt out of the
 *  safety net exactly when it matters most), so this package refuses to
 *  make that mistake on the caller's behalf. */
export function ifMatchHeaders(
  cache: Map<string, number>,
  resource: string,
  id: unknown,
  config: ResourceConfig,
  override?: number,
): HeadersInit {
  if (!config.versionField) return {};
  const version = override ?? cache.get(versionKey(resource, id));
  if (version === undefined) {
    throw new Error(
      `no known version for ${resource}/${String(id)} — call getOne (or getList) before ` +
        "update/delete on a @version model, or pass meta: { ifMatch: <version> } explicitly",
    );
  }
  return { "If-Match": `"${version}"` };
}
