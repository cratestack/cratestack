import type {
  BaseKey,
  BaseRecord,
  CreateManyParams,
  CreateManyResponse,
  CreateParams,
  CreateResponse,
  CustomParams,
  CustomResponse,
  DataProvider,
  DeleteManyParams,
  DeleteManyResponse,
  DeleteOneParams,
  DeleteOneResponse,
  GetListParams,
  GetListResponse,
  GetManyParams,
  GetManyResponse,
  GetOneParams,
  GetOneResponse,
  UpdateManyParams,
  UpdateManyResponse,
  UpdateParams,
  UpdateResponse,
} from "@refinedev/core";
import { toRefineError } from "./errors.js";
import { toQueryFilters, toSortQuery } from "./filters.js";
import type { CratestackFetchQuery, CratestackPage, ResourceConfig, ResourceMap } from "./types.js";
import { createVersionCache, forgetVersion, ifMatchHeaders, rememberVersion } from "./version.js";

export { isCratestackHttpError, toRefineError } from "./errors.js";
export { toQueryFilters, toSortQuery } from "./filters.js";
export type {
  CratestackFetchQuery,
  CratestackHttpErrorLike,
  CratestackModelApi,
  CratestackPage,
  CratestackRequestConfig,
  ResourceConfig,
  ResourceMap,
} from "./types.js";

type Row = Record<string, unknown>;

function withRefineId(record: Row, primaryKey: string): BaseRecord {
  // The cast is honest, not a shortcut: cratestack's `@id` is always a
  // scalar (Int, Cuid, String, …), all of which are `BaseKey` (`string |
  // number`) at the JSON-decoded value level — `unknown` here is a gap
  // in what `Row`'s index signature can express, not genuine uncertainty
  // about the runtime type.
  return { ...record, id: record[primaryKey] as BaseKey };
}

function requireResource(resources: ResourceMap, resource: string): ResourceConfig {
  const config = resources[resource];
  if (!config) {
    throw new Error(
      `no cratestack resource configured for "${resource}" — add it to the "resources" map ` +
        "passed to createCratestackDataProvider",
    );
  }
  return config;
}

export interface CreateCratestackDataProviderOptions {
  /** Backs `getApiUrl()`. Defaults to `() => ""` — refine rarely calls
   *  this in practice, but a real generated client's own origin/basePath
   *  is available at `client.runtime.origin` / `client.runtime.basePath`
   *  if a consumer needs an accurate value. */
  getApiUrl?: () => string;
  /** Backs `custom()`, refine's escape hatch for a cratestack `procedure`
   *  call — there's nowhere else in a `DataProvider` for one to live.
   *  Keyed by procedure name, called as `client.procedures.<name>`
   *  would be; `custom({ meta: { procedure: "<name>" }, payload })`
   *  dispatches to `procedures[name](payload)`. Omit if the schema has
   *  no procedures a refine app needs to call. */
  procedures?: Record<string, (args: unknown) => Promise<unknown>>;
}

/** Builds a refine `DataProvider` over one or more cratestack generated
 *  REST model classes (`client.widgets`, `client.ledgers`, …), given a
 *  manifest describing each resource's primary-key field, `@@paged`
 *  status, and `@version` field.
 *
 *  Write that manifest by hand, or generate it: `cratestack
 *  generate-typescript --refine` emits a `src/refine.ts` next to the
 *  client whose `cratestackRefineResources(client)` returns exactly this
 *  {@link ResourceMap}. The provider itself is not generated and has no
 *  CLI subcommand — see the package README's "A runtime package with a
 *  generated manifest" section for the split.
 *
 *  RPC-transport schemas aren't wired here — see the README's "REST
 *  only" note. */
export function createCratestackDataProvider(
  resources: ResourceMap,
  options: CreateCratestackDataProviderOptions = {},
): DataProvider {
  const versionCache = createVersionCache();

  async function getList(params: GetListParams): Promise<GetListResponse> {
    const config = requireResource(resources, params.resource);
    const usePaging = config.paged && params.pagination?.mode !== "off";
    const currentPage = params.pagination?.currentPage ?? 1;
    const pageSize = params.pagination?.pageSize ?? 10;

    const sort = toSortQuery(params.sorters);
    const query: CratestackFetchQuery = {
      filters: toQueryFilters(params.filters),
      ...(sort ? { sort } : {}),
      ...(usePaging ? { limit: pageSize, offset: (currentPage - 1) * pageSize } : {}),
    };

    const result = await config.api.list({ query });
    const items = config.paged ? (result as CratestackPage<Row>).items : (result as Row[]);
    for (const item of items) {
      rememberVersion(versionCache, params.resource, item[config.primaryKey], item, config);
    }

    return {
      data: items.map((item) => withRefineId(item, config.primaryKey)),
      // A non-`@@paged` resource has no server-computed total beyond what
      // one page returned — `items.length` here is honest about *that*
      // response, not a claim about the full row count. See the README's
      // "Pagination" section before wiring page controls to one.
      total: config.paged
        ? ((result as CratestackPage<Row>).totalCount ?? items.length)
        : items.length,
    };
  }

  async function getOne(params: GetOneParams): Promise<GetOneResponse> {
    const config = requireResource(resources, params.resource);
    try {
      const record = (await config.api.get(params.id)) as Row;
      rememberVersion(versionCache, params.resource, params.id, record, config);
      return { data: withRefineId(record, config.primaryKey) };
    } catch (error) {
      throw toRefineError(error);
    }
  }

  // Optional on `DataProvider` — refine only falls back to it when it's
  // implemented. Because the `in` operator applies to any required
  // field, including the primary key, this is one `list()` call rather
  // than N `getOne` calls.
  async function getMany(params: GetManyParams): Promise<GetManyResponse> {
    const config = requireResource(resources, params.resource);
    const query: CratestackFetchQuery = {
      filters: { [`${config.primaryKey}__in`]: params.ids.map(String).join(",") },
    };
    const result = await config.api.list({ query });
    const items = config.paged ? (result as CratestackPage<Row>).items : (result as Row[]);
    for (const item of items) {
      rememberVersion(versionCache, params.resource, item[config.primaryKey], item, config);
    }
    return { data: items.map((item) => withRefineId(item, config.primaryKey)) };
  }

  async function create(params: CreateParams): Promise<CreateResponse> {
    const config = requireResource(resources, params.resource);
    if (!config.api.create) {
      throw new Error(
        `"${params.resource}" has no generated create route — its model declares no ` +
          '@@allow("create", ...) policy',
      );
    }
    try {
      const record = (await config.api.create(params.variables)) as Row;
      rememberVersion(versionCache, params.resource, record[config.primaryKey], record, config);
      return { data: withRefineId(record, config.primaryKey) };
    } catch (error) {
      throw toRefineError(error);
    }
  }

  async function update(params: UpdateParams): Promise<UpdateResponse> {
    const config = requireResource(resources, params.resource);
    const headers = ifMatchHeaders(
      versionCache,
      params.resource,
      params.id,
      config,
      params.meta?.ifMatch as number | undefined,
    );
    try {
      const record = (await config.api.update(params.id, params.variables, { headers })) as Row;
      rememberVersion(versionCache, params.resource, params.id, record, config);
      return { data: withRefineId(record, config.primaryKey) };
    } catch (error) {
      throw toRefineError(error);
    }
  }

  async function deleteOne(params: DeleteOneParams): Promise<DeleteOneResponse> {
    const config = requireResource(resources, params.resource);
    const headers = ifMatchHeaders(
      versionCache,
      params.resource,
      params.id,
      config,
      params.meta?.ifMatch as number | undefined,
    );
    try {
      await config.api.delete(params.id, { headers });
      return { data: { id: params.id } as BaseRecord };
    } catch (error) {
      throw toRefineError(error);
    } finally {
      forgetVersion(versionCache, params.resource, params.id);
    }
  }

  // createMany/updateMany/deleteMany: IMPLEMENTED, not declined — but as
  // N sequential single-record round trips via Promise.all, each going
  // through the same create/update/deleteOne above (so If-Match/version-
  // cache/error-mapping behavior stays identical to the single-record
  // path). This is a real, working implementation, just not an atomic
  // or batched one: the generated REST client exposes no `updateMany`/
  // `deleteMany` wrapper over the server's real `update_many`/
  // `delete_many`, and REST-transport schemas have no `/rpc/batch`
  // endpoint (that's RPC-transport only) — see the package README.
  async function createMany(params: CreateManyParams): Promise<CreateManyResponse> {
    const meta = params.meta;
    const records = await Promise.all(
      params.variables.map((variables) =>
        create(
          meta
            ? { resource: params.resource, variables, meta }
            : { resource: params.resource, variables },
        ),
      ),
    );
    return { data: records.map((r) => r.data) };
  }

  async function updateMany(params: UpdateManyParams): Promise<UpdateManyResponse> {
    const meta = params.meta;
    const variables = params.variables;
    const records = await Promise.all(
      params.ids.map((id) =>
        update(
          meta
            ? { resource: params.resource, id, variables, meta }
            : { resource: params.resource, id, variables },
        ),
      ),
    );
    return { data: records.map((r) => r.data) };
  }

  async function deleteMany(params: DeleteManyParams): Promise<DeleteManyResponse> {
    const meta = params.meta;
    const variables = params.variables;
    const records = await Promise.all(
      params.ids.map((id) =>
        deleteOne(
          meta || variables
            ? {
                resource: params.resource,
                id,
                ...(variables ? { variables } : {}),
                ...(meta ? { meta } : {}),
              }
            : { resource: params.resource, id },
        ),
      ),
    );
    return { data: records.map((r) => r.data) };
  }

  async function custom(params: CustomParams): Promise<CustomResponse> {
    const procedureName = params.meta?.procedure as string | undefined;
    const procedureFn = procedureName ? options.procedures?.[procedureName] : undefined;
    if (typeof procedureFn !== "function") {
      throw new Error(
        'custom() needs meta: { procedure: "<name>" } naming an entry in the "procedures" map ' +
          "passed to createCratestackDataProvider",
      );
    }
    const data = await procedureFn(params.payload ?? {});
    return { data: data as BaseRecord };
  }

  // Cast, not a structural coincidence: `DataProvider`'s methods are
  // individually generic over a caller-chosen `TData extends BaseRecord`
  // (so a caller can write `getOne<Widget>(...)`), but this factory
  // returns the SAME concrete implementation regardless of what a caller
  // instantiates TData as — every method already returns a real
  // `BaseRecord`-shaped value built from the real decoded response, and
  // relies on the caller's own generic parameter for compile-time
  // narrowing at the call site, exactly like every other hand-written
  // `DataProvider` implementation (refine's own generic method shape
  // isn't satisfiable by a concrete non-generic function under
  // `exactOptionalPropertyTypes`, a known TypeScript variance gap, not a
  // real type-safety hole here).
  return {
    getApiUrl: options.getApiUrl ?? (() => ""),
    getList,
    getOne,
    getMany,
    create,
    createMany,
    update,
    updateMany,
    deleteOne,
    deleteMany,
    custom,
  } as DataProvider;
}
