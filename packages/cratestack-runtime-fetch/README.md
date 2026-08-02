# @cratestack/runtime-fetch

A `typeof fetch`-compatible transport for CrateStack's generated TypeScript RPC client
(`transport rpc` schemas). With no options it's byte-identical to the global `fetch`; its only
behavior is an optional per-call `timeoutMs`.

## Usage

```ts
import { createFetchRuntime } from "@cratestack/runtime-fetch";
import { CratestackRpcRuntime } from "./generated/runtime"; // your project's generated client

const runtime = new CratestackRpcRuntime("https://api.example.com", {
  fetch: createFetchRuntime({ timeoutMs: 10_000 }),
});
```

A timeout fires an `AbortSignal.timeout`-style abort; if the call already carries its own
`AbortSignal` (e.g. `CratestackRpcCallOptions.signal`), either one aborting cancels the request —
neither overrides the other.
