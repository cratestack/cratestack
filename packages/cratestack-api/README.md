# @cratestack/api

Compat umbrella over the split `@cratestack/*` npm family for CrateStack's generated TypeScript
RPC client (`transport rpc` schemas). If you're starting fresh, prefer depending on the individual
packages below directly — smaller install, no unused peer dependencies. `@cratestack/api` exists
so existing `import { createBatchLink } from "@cratestack/api"` code keeps working unchanged.

## The split

| Package | What it does |
| --- | --- |
| [`@cratestack/ts-types`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-ts-types) | Shared `RpcLink`/wire-frame interfaces. Types only — no runtime code. |
| [`@cratestack/link-batch`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-link-batch) | Automatic batch-scheduler `RpcLink`. |
| [`@cratestack/link-logger`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-link-logger) | Reference logging `RpcLink`. |
| [`@cratestack/runtime-fetch`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-runtime-fetch) | `typeof fetch` transport with an optional timeout. |
| [`@cratestack/runtime-axios`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-runtime-axios) | `typeof fetch` transport backed by axios. |
| [`@cratestack/validator-zod`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-validator-zod) | Input-validating `RpcLink`, zod schemas. |
| [`@cratestack/validator-yup`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-validator-yup) | Input-validating `RpcLink`, yup schemas. |
| [`@cratestack/adapter-tanstack-query`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-adapter-tanstack-query) | Generic TanStack Query option builders. |
| [`@cratestack/adapter-rtk`](https://github.com/cratestack/cratestack/tree/main/packages/cratestack-adapter-rtk) | RTK Query `BaseQueryFn` adapter. |

## Usage

The root import is unchanged from before the split — it re-exports exactly `ts-types` +
`link-batch` + `link-logger`, so importing it never pulls in zod/yup/axios/tanstack-query/rtk as
implicit peer dependencies:

```ts
import { createBatchLink, createLoggerLink } from "@cratestack/api";
import { CratestackRpcRuntime } from "./generated/runtime"; // your project's generated client

const runtime = new CratestackRpcRuntime("https://api.example.com", {
  links: [createLoggerLink(), createBatchLink()],
});
const client = new MyGeneratedClient(runtime);

// These three calls, if issued in the same tick, become ONE /rpc/batch request:
const [a, b, c] = await Promise.all([
  client.widgets.get(1),
  client.widgets.get(2),
  client.widgets.get(3),
]);
```

Everything else added since the split is available as a named subpath — each pulls in only its
own peer dependency, not every package's:

```ts
import { createFetchRuntime } from "@cratestack/api/runtime-fetch";
import { createAxiosRuntime } from "@cratestack/api/runtime-axios";
import { createZodValidatorLink } from "@cratestack/api/validator-zod";
import { createYupValidatorLink } from "@cratestack/api/validator-yup";
import { rpcQueryOptions, rpcMutationOptions } from "@cratestack/api/adapter-tanstack-query";
import { createRpcBaseQuery } from "@cratestack/api/adapter-rtk";
```

See each package's own README (linked in the table above) for its full API and semantics.

Order matters: a link's `next` reruns everything below it (the real fetch and any links declared
after it), never just the terminal fetch. `stream()` calls bypass the chain entirely — a link that
wants to inspect/retry a response would need to clone a streamed body, which defeats streaming.
