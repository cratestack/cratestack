import type { RpcCaller } from "@cratestack/ts-types";
import type { QueryKey } from "@tanstack/query-core";

export type { RpcCaller } from "@cratestack/ts-types";

/** Rewrites every `bigint` in `value` into its canonical decimal string and
 *  copies the arrays and plain objects it walks through; any other value
 *  (a `Decimal`, a `Uint8Array`, a `Date`) is returned as it came.
 *
 *  A `BigInt` field of a generated client is a `bigint` (ADR 0019), and
 *  TanStack Query hashes a query key with `JSON.stringify`, which throws
 *  "Do not know how to serialize a BigInt" on one. The decimal string is the
 *  same text the client puts on the wire, so two different `bigint`s always
 *  give two different keys (`2n ** 53n` and `2n ** 53n + 1n` stay apart),
 *  where a `number` would round them together. */
function encodeKeyValue(value: unknown): unknown {
  if (typeof value === "bigint") {
    return value.toString();
  }
  if (Array.isArray(value)) {
    return value.map(encodeKeyValue);
  }
  if (isPlainObject(value)) {
    const encoded: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value)) {
      encoded[key] = encodeKeyValue(entry);
    }
    return encoded;
  }
  return value;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/** The `[opId, input]` tuple every helper below keys its query/mutation
 *  on — exported so callers can `queryClient.invalidateQueries({ queryKey: rpcQueryKey(opId, input) })`
 *  without re-deriving the same shape by hand.
 *
 *  A `bigint` anywhere in `input` (a `BigInt` id, a `BigInt` filter value, a
 *  computed parameter) is stored in the key as its canonical decimal string,
 *  so the key hashes. Build the key you pass to `invalidateQueries`,
 *  `setQueryData` and friends with this function too: a hand-written key
 *  holding the raw `bigint` neither matches the stored one nor hashes. */
export function rpcQueryKey(opId: string, input?: unknown): QueryKey {
  return input === undefined ? [opId] : [opId, encodeKeyValue(input)];
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
