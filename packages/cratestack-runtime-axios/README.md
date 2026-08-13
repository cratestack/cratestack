# @cratestack/runtime-axios

A `typeof fetch`-compatible transport for CrateStack's generated TypeScript RPC client
(`transport rpc` schemas), backed by [axios](https://axios-http.com/) instead of the global
`fetch` — useful for environments where axios is already the house HTTP client (interceptors,
proxy/agent config, request/response transforms you don't want to reimplement).

## Usage

```ts
import { createAxiosRuntime } from "@cratestack/runtime-axios";
import { CratestackRpcRuntime } from "./generated/runtime"; // your project's generated client
import myAxiosInstance from "./my-axios-instance";

const runtime = new CratestackRpcRuntime("https://api.example.com", {
  fetch: createAxiosRuntime({ instance: myAxiosInstance }), // defaults to the `axios` singleton
});
```

Every request is issued with `validateStatus: () => true` — a non-2xx response comes back as a
normal `Response` (so `CratestackRpcRuntime`/`RpcLink`s can inspect `response.status` and decode
the server's `RpcErrorBody` themselves) rather than axios throwing an `AxiosError`.
