import type { BaseQueryFn } from "@reduxjs/toolkit/query";

/** Structural match for `CratestackRpcRuntime.call()` (see
 *  `crates/cratestack-client-typescript/templates/src/rpc-runtime.ts.j2`)
 *  — a generated client exposes this as its public `.runtime` field, so
 *  `createRpcBaseQuery(client.runtime)` works against any generated
 *  project without this package importing its (per-project, unshared)
 *  class. */
export interface RpcCaller {
  call<I, O>(opId: string, input: I, options?: { signal?: AbortSignal }): Promise<O>;
}

/** A single RTK Query endpoint request — the `args` shape every
 *  endpoint defined against {@link createRpcBaseQuery} passes to
 *  `query()`/`queryFn()`. */
export interface RpcBaseQueryArgs {
  opId: string;
  input?: unknown;
}

/** The error shape RTK Query endpoints see in `error` on a failed
 *  call — distinguishes a real server-side `RpcErrorBody` ("RPC_ERROR",
 *  `data` carries the decoded body) from a transport-level failure
 *  ("RPC_TRANSPORT_ERROR", no structured body). */
export interface RpcBaseQueryError {
  status: "RPC_ERROR" | "RPC_TRANSPORT_ERROR";
  error: string;
  data?: unknown;
}

/** Adapts an `RpcCaller` (a generated client's `.runtime`, or the
 *  runtime itself) into an RTK Query `BaseQueryFn`, so
 *  `createApi({ baseQuery: createRpcBaseQuery(client.runtime) })`
 *  dispatches every endpoint through the same `RpcLink` chain
 *  (`@cratestack/link-batch`, `@cratestack/link-logger`, etc.) the rest
 *  of the generated client uses — instead of RTK Query's default
 *  `fetchBaseQuery` reimplementing the wire protocol. */
export function createRpcBaseQuery(
  client: RpcCaller,
): BaseQueryFn<RpcBaseQueryArgs, unknown, RpcBaseQueryError> {
  return async ({ opId, input }, api) => {
    try {
      const data = await client.call(opId, input ?? null, { signal: api.signal });
      return { data };
    } catch (error) {
      return { error: toBaseQueryError(error) };
    }
  };
}

function toBaseQueryError(error: unknown): RpcBaseQueryError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    const body = error as { code: string; message: string; details?: unknown };
    return { status: "RPC_ERROR", error: body.message, data: body };
  }
  return {
    status: "RPC_TRANSPORT_ERROR",
    error: error instanceof Error ? error.message : String(error),
  };
}
