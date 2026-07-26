# @cratestack/api

Composable links for CrateStack's generated TypeScript RPC client (`transport rpc` schemas).

Every `cratestack generate-typescript` project ships a `links?: RpcLink[]` option on
`CratestackRpcClientOptions` ([issue #182](https://github.com/cratestack/cratestack/issues/182)) —
an ordered chain of interceptors, each wrapping the next, terminating in the real network call.
This package ships two of them:

- **`createBatchLink()`** — a [batshit](https://github.com/yornaath/batshit)-style automatic batch
  scheduler. Multiple unary calls issued in the same tick collapse into a single `POST /rpc/batch`
  request instead of firing one `POST /rpc/{op_id}` each.
- **`createLoggerLink()`** — a small reference link that logs each call's op id, outcome, and
  duration.

## Usage

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

Order matters: a link's `next` reruns everything below it (the real fetch and any links declared
after it), never just the terminal fetch. Put `createBatchLink()` last (closest to the network) if
you also want a logger or retry link to see the real per-call outcome; put it first if you want
logging/retry to apply to the *batched* request instead.

`stream()` calls bypass the chain entirely — a link that wants to inspect/retry a response would
need to clone a streamed body, which defeats streaming.

## Batching semantics

- **Window**: defaults to `queueMicrotask` — calls fired synchronously in the same tick (e.g.
  inside `Promise.all`) collapse. Pass `windowMs` to widen the window across ticks.
- **Dedup**: only calls sharing an explicit `idempotencyKey` are collapsed into one request frame
  by default — that's already the caller's own signal that the call is a safe repeat. Calls with
  no idempotency key are never auto-collapsed, since the server does no dedup of its own and
  silently merging two textually-identical but unmarked mutations would be unsafe. Pass a custom
  `dedupe` for full value-based collapsing, or `() => null` to disable dedup and only batch.
- **Correlation**: results are fanned back out by matching each response frame's `id`, not array
  position — the server's `/rpc/batch` contract already guarantees order (see
  `docs/design/rpc-transport.md` §3.2), but id-based matching is strictly more robust and mirrors
  the repo's own Rust client-side batch debouncer (`examples/rpc-batch-debounce`).

### Known limitations

- The synthesized `/rpc/batch` request reuses the **first** queued call's `headers`/`fetchFn`/
  `codec` for the whole flush — per-call custom headers on later calls in the same window are not
  applied to the aggregate request. Pass shared headers via the runtime's own `headers` option
  rather than per-call `CratestackRpcCallOptions.headers` when using this link.
- Aborting an individual call's `AbortSignal` only cancels it if its batch hasn't been sent yet —
  it does not cancel an in-flight `/rpc/batch` request.
