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
