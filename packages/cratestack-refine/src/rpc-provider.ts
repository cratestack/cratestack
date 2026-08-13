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
import { toRpcQueryFilters, toRpcSortQuery } from "./rpc-filters.js";
import { runCreateMany, runDeleteMany, runUpdateMany } from "./rpc-many.js";
import type { CratestackRpcListQuery, RpcResourceConfig, RpcResourceMap } from "./rpc-types.js";
import type { CratestackPage } from "./types.js";
import { createVersionCache, forgetVersion, ifMatchHeaders, rememberVersion } from "./version.js";

type Row = Record<string, unknown>;

function withRefineId(record: Row, primaryKey: string): BaseRecord {
  // Same honest cast as REST's `withRefineId` in `index.ts` — see that
  // function's doc comment.
  return { ...record, id: record[primaryKey] as BaseKey };
}

function requireResource(resources: RpcResourceMap, resource: string): RpcResourceConfig {
  const config = resources[resource];
  if (!config) {
    throw new Error(
      `no cratestack resource configured for "${resource}" — add it to the "resources" map ` +
        "passed to createCratestackRpcDataProvider",
    );
  }
  return config;
}

export interface CreateCratestackRpcDataProviderOptions {
  /** Backs `getApiUrl()`. Defaults to `() => ""`, same as the REST
   *  provider — see `CreateCratestackDataProviderOptions.getApiUrl` in
   *  `index.ts` for why refine rarely calls this in practice. */
  getApiUrl?: () => string;
  /** Backs `custom()` — identical contract to the REST provider's
   *  `procedures` option (`CreateCratestackDataProviderOptions.procedures`
   *  in `index.ts`): keyed by procedure name, called as
   *  `client.procedures.<name>` would be. */
  procedures?: Record<string, (args: unknown) => Promise<unknown>>;
}

/** Builds a refine `DataProvider` over one or more `transport rpc`
 *  cratestack generated model classes (`client.widgets`, `client.ledgers`,
 *  …) — the RPC sibling of `createCratestackDataProvider` in `index.ts`.
 *  Same manifest shape (`RpcResourceMap`), same filter/pagination/
 *  primary-key/optimistic-locking semantics; only the wire calls
 *  underneath differ, because the generated RPC client's `list` takes its
 *  query positionally instead of nested in an options object (see
 *  `rpc-types.ts`'s `CratestackRpcModelApi` doc comment).
 *
 *  Deliberately uses only unary `POST /rpc/{op_id}` calls, never
 *  `POST /rpc/batch` — a batch request is a single HTTP request carrying
 *  N frames, so a per-frame `If-Match` header (what `@version` optimistic
 *  locking needs) is not expressible there. `createMany`/`updateMany`/
 *  `deleteMany` below are N real unary round trips for exactly that
 *  reason, same as REST's non-atomic N-round-trip strategy. */
export function createCratestackRpcDataProvider(
  resources: RpcResourceMap,
  options: CreateCratestackRpcDataProviderOptions = {},
): DataProvider {
  const versionCache = createVersionCache();

  async function getList(params: GetListParams): Promise<GetListResponse> {
    const config = requireResource(resources, params.resource);
    const usePaging = config.paged && params.pagination?.mode !== "off";
    const currentPage = params.pagination?.currentPage ?? 1;
    const pageSize = params.pagination?.pageSize ?? 10;

    const sort = toRpcSortQuery(params.sorters);
    const query: CratestackRpcListQuery = {
      filters: toRpcQueryFilters(params.filters),
      ...(sort ? { sort } : {}),
      ...(usePaging ? { limit: pageSize, offset: (currentPage - 1) * pageSize } : {}),
    };

    const result = await config.api.list(query);
    const items = config.paged ? (result as CratestackPage<Row>).items : (result as Row[]);
    for (const item of items) {
      rememberVersion(versionCache, params.resource, item[config.primaryKey], item, config);
    }

    return {
      data: items.map((item) => withRefineId(item, config.primaryKey)),
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

  // Same one-list-call-instead-of-N-getOne-calls strategy as REST's
  // `getMany` — see that function's comment in `index.ts`.
  async function getMany(params: GetManyParams): Promise<GetManyResponse> {
    const config = requireResource(resources, params.resource);
    const query: CratestackRpcListQuery = {
      filters: [{ key: `${config.primaryKey}__in`, value: params.ids.map(String).join(",") }],
    };
    const result = await config.api.list(query);
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

  async function createMany(params: CreateManyParams): Promise<CreateManyResponse> {
    return runCreateMany(params, create);
  }

  async function updateMany(params: UpdateManyParams): Promise<UpdateManyResponse> {
    return runUpdateMany(params, update);
  }

  async function deleteMany(params: DeleteManyParams): Promise<DeleteManyResponse> {
    return runDeleteMany(params, deleteOne);
  }

  async function custom(params: CustomParams): Promise<CustomResponse> {
    const procedureName = params.meta?.procedure as string | undefined;
    const procedureFn = procedureName ? options.procedures?.[procedureName] : undefined;
    if (typeof procedureFn !== "function") {
      throw new Error(
        'custom() needs meta: { procedure: "<name>" } naming an entry in the "procedures" map ' +
          "passed to createCratestackRpcDataProvider",
      );
    }
    const data = await procedureFn(params.payload ?? {});
    return { data: data as BaseRecord };
  }

  // Same cast, same reasoning as REST's `createCratestackDataProvider` —
  // see that function's closing comment in `index.ts`.
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
