import type { QueryKey } from "@tanstack/query-core";

/** Structural match for `CratestackRpcRuntime.call()` (see
 *  `crates/cratestack-client-typescript/templates/src/rpc-runtime.ts.j2`)
 *  — a generated client exposes this as its public `.runtime` field, so
 *  `rpcQueryOptions(client.runtime, opId, input)` works against any
 *  generated project without this package importing its (per-project,
 *  unshared) class. */
export interface RpcCaller {
  call<I, O>(opId: string, input: I, options?: { signal?: AbortSignal }): Promise<O>;
}

/** The `[opId, input]` tuple every helper below keys its query/mutation
 *  on — exported so callers can `queryClient.invalidateQueries({ queryKey: rpcQueryKey(opId, input) })`
 *  without re-deriving the same shape by hand. */
export function rpcQueryKey(opId: string, input?: unknown): QueryKey {
  return input === undefined ? [opId] : [opId, input];
}

/** Builds a `{ queryKey, queryFn }` pair for `useQuery` (or its
 *  vue-query/solid-query/svelte-query equivalents — all built on
 *  `@tanstack/query-core`) from a single unary RPC call. Framework-
 *  agnostic: pass the result straight through, or spread it alongside
 *  your own `staleTime`/`enabled`/etc. */
export function rpcQueryOptions<I, O>(
  client: RpcCaller,
  opId: string,
  input: I,
): { queryKey: QueryKey; queryFn: (context: { signal: AbortSignal }) => Promise<O> } {
  return {
    queryKey: rpcQueryKey(opId, input),
    queryFn: ({ signal }) => client.call<I, O>(opId, input, { signal }),
  };
}

/** Builds a `{ mutationKey, mutationFn }` pair for `useMutation`. The
 *  input is supplied at call time (`mutate(input)`), not here — mirrors
 *  the generated `useCreateXMutation`/`useUpdateXMutation` hooks'
 *  shape. */
export function rpcMutationOptions<I, O>(
  client: RpcCaller,
  opId: string,
): { mutationKey: QueryKey; mutationFn: (input: I) => Promise<O> } {
  return {
    mutationKey: [opId],
    mutationFn: (input: I) => client.call<I, O>(opId, input),
  };
}

/** True when `error` is a `CratestackRpcError`-shaped value (or a
 *  decoded `RpcErrorBody`) whose `code` matches — structural, not an
 *  `instanceof` check, since `CratestackRpcError` is a per-project
 *  generated class with no shared import path. Useful in
 *  `retry`/`throwOnError` callbacks, e.g. never retrying a
 *  `"not_found"`. */
export function isRpcErrorCode(error: unknown, code: string): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code: unknown }).code === code
  );
}
