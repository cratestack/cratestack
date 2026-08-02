# @cratestack/link-batch

A [batshit](https://github.com/yornaath/batshit)-style automatic batch scheduler for CrateStack's
generated TypeScript RPC client (`transport rpc` schemas), shipped as an
[`RpcLink`](https://github.com/cratestack/cratestack/blob/main/packages/cratestack-ts-types)
([issue #182](https://github.com/cratestack/cratestack/issues/182)) rather than a `fetch`
override — so it composes with `@cratestack/link-logger`, a retry link, or an auth-refresh link
instead of clobbering them.

Multiple unary calls issued in the same tick collapse into a single `POST /rpc/batch` request
instead of firing one `POST /rpc/{op_id}` each.

## Usage

```ts
import { createBatchLink } from "@cratestack/link-batch";
import { CratestackRpcRuntime } from "./generated/runtime"; // your project's generated client

const runtime = new CratestackRpcRuntime("https://api.example.com", {
  links: [createBatchLink()],
});
const client = new MyGeneratedClient(runtime);

// These three calls, if issued in the same tick, become ONE /rpc/batch request:
const [a, b, c] = await Promise.all([
  client.widgets.get(1),
  client.widgets.get(2),
  client.widgets.get(3),
]);
```

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
