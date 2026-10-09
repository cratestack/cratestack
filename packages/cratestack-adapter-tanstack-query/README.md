# @cratestack/adapter-tanstack-query

Generic [TanStack Query](https://tanstack.com/query) option builders over CrateStack's generated
TypeScript RPC client (`transport rpc` schemas), for hand-written query/mutation hooks that don't
go through the fully-generated `use{Model}Query`/`use{Model}Mutation` hooks
(`cratestack generate-typescript`'s own `rpc-react-query.ts.j2` output) — e.g. calling a
`procedure` the generated hooks don't cover yet, or using vue-query/solid-query/svelte-query
instead of `@tanstack/react-query`.

Framework-agnostic: everything here is typed against `@tanstack/query-core`, which every TanStack
Query framework binding builds on.

## Usage

```ts
import { rpcQueryOptions, rpcMutationOptions, isRpcErrorCode } from "@cratestack/adapter-tanstack-query";
import { useQuery, useMutation } from "@tanstack/react-query";
import { client } from "./generated/client"; // your project's generated client instance

function useWidget(id: number) {
  return useQuery({
    ...rpcQueryOptions(client.runtime, "model.Widget.get", { id }),
    retry: (failureCount, error) => !isRpcErrorCode(error, "not_found") && failureCount < 3,
  });
}

function useCreateOrder() {
  return useMutation(rpcMutationOptions(client.runtime, "model.Order.create"));
}
```

`rpcQueryOptions`/`rpcMutationOptions` take an `RpcCaller` — any object with a
`call<I, O>(opId, input, options?)` method, which is exactly the shape of a generated client's
public `.runtime` field (`CratestackRpcRuntime`). Requests issued this way go through the same
`links` chain (`@cratestack/link-batch`, `@cratestack/link-logger`, etc.) as every other call on
that runtime.

## `BigInt` fields

A `BigInt` field of a generated client is a `bigint` (ADR 0019), and TanStack Query hashes a
query key with `JSON.stringify`, which throws on one ("Do not know how to serialize a BigInt").
`rpcQueryKey`, and so `rpcQueryOptions`, stores every `bigint` in the input, at any depth, as its
canonical decimal string: the same text the client sends on the wire. Two different values are two
keys (`2n ** 53n` and `2n ** 53n + 1n` stay apart, where a `number` would round them together) and
the same value is the same key. The request itself still gets the real `bigint`; the generated
runtime encodes it.

```ts
useQuery(rpcQueryOptions(client.runtime, "model.Counter.get", { id: 9007199254740993n }));

// Build the key you invalidate with through the same function: a hand-written
// key holding a raw bigint neither hashes nor matches the stored one.
queryClient.invalidateQueries({
  queryKey: rpcQueryKey("model.Counter.get", { id: 9007199254740993n }),
});
```
