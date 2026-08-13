# @cratestack/link-logger

A small reference [`RpcLink`](https://github.com/cratestack/cratestack/blob/main/packages/cratestack-ts-types)
([issue #182](https://github.com/cratestack/cratestack/issues/182)) for CrateStack's generated
TypeScript RPC client (`transport rpc` schemas) that logs each call's kind, op id, outcome, and
duration. Never touches `response.body`.

## Usage

```ts
import { createLoggerLink } from "@cratestack/link-logger";
import { CratestackRpcRuntime } from "./generated/runtime"; // your project's generated client

const runtime = new CratestackRpcRuntime("https://api.example.com", {
  links: [createLoggerLink()],
});
```

Pass a custom logger (anything with `info`/`error` methods, e.g. `pino` or `winston`) instead of
the default `console`:

```ts
createLoggerLink(myPinoLogger);
```

Order matters: put `createLoggerLink()` before `@cratestack/link-batch` in the `links` array if
you want it to see the *batched* request, or after it if you want it to see each real per-call
outcome — see that package's README for the composition details.
