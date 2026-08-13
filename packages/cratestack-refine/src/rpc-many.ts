import type {
  CreateManyParams,
  CreateManyResponse,
  CreateParams,
  CreateResponse,
  DeleteManyParams,
  DeleteManyResponse,
  DeleteOneParams,
  DeleteOneResponse,
  UpdateManyParams,
  UpdateManyResponse,
  UpdateParams,
  UpdateResponse,
} from "@refinedev/core";

/** `createMany`/`updateMany`/`deleteMany`, factored out of
 *  `rpc-provider.ts` because the N-sequential-round-trips-via-`Promise.all`
 *  strategy is entirely refine-shaped, not RPC-shaped — it just replays
 *  the single-record `create`/`update`/`deleteOne` closure N times, so it
 *  doesn't need to know which transport those closures talk over. Same
 *  strategy REST's `createCratestackDataProvider` uses inline (not
 *  reused from here, to keep this change additive rather than touching
 *  already-shipped REST code) — see that file's own comment for why this
 *  is IMPLEMENTED, not declined: real behavior, just not atomic or
 *  batched (`/rpc/batch` exists on this transport, but batching would
 *  make per-frame `If-Match` inexpressible — see `rpc-provider.ts`). */
export async function runCreateMany(
  params: CreateManyParams,
  create: (p: CreateParams) => Promise<CreateResponse>,
): Promise<CreateManyResponse> {
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

export async function runUpdateMany(
  params: UpdateManyParams,
  update: (p: UpdateParams) => Promise<UpdateResponse>,
): Promise<UpdateManyResponse> {
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

export async function runDeleteMany(
  params: DeleteManyParams,
  deleteOne: (p: DeleteOneParams) => Promise<DeleteOneResponse>,
): Promise<DeleteManyResponse> {
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
