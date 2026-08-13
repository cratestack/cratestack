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
- **Partitioning** (fixed in [#273](https://github.com/cratestack/cratestack/issues/273); versions
  before this carried a bug here — see "Known limitations" below): each flush is split into
  partitions by transport envelope — headers (excluding `Idempotency-Key`, which is carried
  per-frame, not per-request), `fetch` reference, codec reference, and the resolved batch URL —
  and every partition is sent as its own `POST /rpc/batch`. Calls that share an envelope, the
  overwhelmingly common case, still collapse into exactly one request; calls that don't (e.g. two
  different `Authorization` headers) each keep their own, in a separate request, rather than one
  silently overwriting the other's.
- **Aggregate headers**: pass `headers` to `createBatchLink({ headers })` to declare headers that
  every synthesized `/rpc/batch` request carries, merged **over** each partition's own — a
  same-named per-call header is overridden by this value, not the other way around. Use this for
  service-level headers (e.g. an API key) rather than per-tenant auth, which should come from
  per-call headers that drive partitioning.
- **`maxBatchSize`** is enforced **per partition**, not globally across the flush — a single
  oversized partition chunks into several concurrent requests; it never borrows headroom from a
  different partition.
- **Dedup**: only calls sharing an explicit `idempotencyKey` are collapsed into one request frame
  by default — that's already the caller's own signal that the call is a safe repeat. Calls with
  no idempotency key are never auto-collapsed, since the server does no dedup of its own and
  silently merging two textually-identical but unmarked mutations would be unsafe. Pass a custom
  `dedupe` for full value-based collapsing, or `() => null` to disable dedup and only batch. Dedup
  runs *within* a partition — it never collapses calls across two different envelopes.
- **Correlation**: results are fanned back out by matching each response frame's `id`, not array
  position — the server's `/rpc/batch` contract already guarantees order (see
  `docs/design/rpc-transport.md` §3.2), but id-based matching is strictly more robust and mirrors
  the repo's own Rust client-side batch debouncer (`examples/rpc-batch-debounce`).
- **Failure isolation**: a partition that fails (network error, non-OK response) only rejects the
  callers queued in *that* partition — it never affects a concurrently in-flight partition.

### Known limitations

- Aborting an individual call's `AbortSignal` only cancels it if its batch hasn't been sent yet —
  it does not cancel an in-flight `/rpc/batch` request.
- **(Fixed in #273, kept here for anyone reading an older version's docs)** Before #273, the
  synthesized `/rpc/batch` request reused the *first* queued call's `headers`/`fetchFn`/`codec` for
  the whole flush — per-call custom headers on later calls in the same window were silently dropped
  from the aggregate request instead of applied. This is why partitioning exists now.
