# RPC request batching/coalescing — transport-agnostic model

Status: **implemented (TypeScript)** — this document is the write-up #181
asks for, produced against the already-shipped `@cratestack/link-batch`
package (`packages/cratestack-link-batch`, merged via #186 and hardened by
#273). It specifies the batching/coalescing model independently of
TypeScript, per #181's acceptance criterion ("written up independently of
TypeScript so it can be evaluated for a Dart/Flutter port without a
redesign"), and evaluates it against Dio's `QueuedInterceptor`/
interceptor+queue model — the ticket's own named Dart reference (epic #207's
risk table: "Explicit AC requiring the model to be evaluated against a
Dio/TanStack Query reference, not just tRPC"). **No code changes.** The
existing TS implementation is confirmed portable to Dio as designed; nothing
here found a reason to reshape it.
Scope: the coalescing model itself, and its mapping onto Dio. A Dart/Flutter
implementation is explicitly out of scope (#181, #207) — this doc is what
makes that follow-up "a straightforward reuse, not a redesign," not the
follow-up itself.
Tracking: #181 (this doc's source ticket), grouped under epic #207 alongside
#183.

## Summary

| Item | Answer |
|---|---|
| Does a transport-agnostic write-up already exist? | No — before this doc, the model existed only as a TS implementation (`@cratestack/link-batch`) plus its README, both written in `RpcLink`/`fetch`/`Promise` terms. |
| Is the TS model portable to Dio without a redesign? | **Yes.** Every piece (queue, flush trigger, envelope partitioning, explicit-key-only dedup, per-frame correlation, per-partition error isolation) maps onto a Dio `Interceptor` that defers `handler.next()` and later resolves/rejects the held handler — the same primitive Dio itself uses for token-refresh interceptors. See §3. |
| Is `QueuedInterceptor` itself the right base class to port onto? | **No, and that's the interesting finding** — `QueuedInterceptor` solves a different problem (serializing requests behind a lock, e.g. auth-refresh). The primitive this model actually needs is plain `Interceptor.onRequest` with a deferred handler, which Dio also supports natively. See §3.1. |
| Code changes required by this write-up? | None. This is a documentation deliverable per #181's own framing. |

## 1. Why write this down separately from the TS package

`packages/cratestack-link-batch/README.md` documents the *shipped behavior*
correctly, but every sentence in it is phrased in terms of `RpcLink`,
`fetch`, `Promise`, `AbortSignal`, and `queueMicrotask` — TypeScript/web
idioms. #181 (and epic #207's risk table) flagged this exact gap: a TS
implementation existing is not the same thing as a transport-agnostic
*model* existing, and without the latter a Dart port risks re-deriving the
design from scratch, possibly getting the unsafe parts (auto-dedup without
an explicit key, batching idempotency-sensitive calls) wrong a second time.

This document extracts the model as six behaviors, stated without reference
to any one language's primitives (§2), then checks each behavior against
Dio specifically — not generically "Dart has queues too," but the actual
`Interceptor`/`QueuedInterceptor` API surface (§3).

## 2. The model, transport-agnostically

The model is a **queueing decorator in front of a unary call primitive**. It
does not change what a unary call *means*; it changes when the underlying
wire request for it is issued and how many logical calls share that wire
request. Six behaviors, independent of any binding:

### 2.1 Queue accumulation

Every eligible call is intercepted before it reaches the transport. Instead
of dispatching immediately, the call's **request payload** and a **pending
result handle** (something a caller is awaiting) are appended to an
in-memory queue. Nothing about the caller's call site changes — it still
looks like "make one call and await the result"; the queueing is invisible
except for latency.

Reference implementation: `packages/cratestack-link-batch/src/index.ts`
lines 78–96 — `queue.push({ request, resolve, reject })` inside a `new
Promise` executor, so the call's own promise is the pending result handle.

### 2.2 Flush trigger (the "window")

A flush is scheduled the first time something lands in an empty queue, and
runs once. Two trigger modes:

- **Same-tick coalescing (default)**: scheduled as a **microtask** — calls
  issued synchronously in the same turn of the event loop (e.g. several
  calls fired inside a fan-out helper before anything is awaited) land in
  the queue before the microtask runs, so they all flush together. This is
  deliberately *not* "next event-loop tick" — it is strictly narrower, and
  fires before any I/O callback could run.
- **Windowed coalescing (opt-in)**: scheduled as a **timer** of a
  caller-supplied duration, widening the window across turns to catch calls
  from unrelated call sites that aren't synchronously co-located.

Reference: `index.ts` lines 41–55 (`scheduleFlush`) — `queueMicrotask` vs.
`setTimeout(run, windowMs)`, gated on whether `windowMs` was supplied.

**Default-off is a hard requirement**, not a style choice: #181's AC says
batching must be opt-in so unbatched call latency is unchanged by default.
The model satisfies this by living entirely inside an add-on link/interceptor
that must be explicitly installed — the unary call path is unmodified when
it isn't.

### 2.3 Partitioning by transport envelope

A flush does not necessarily become *one* wire request — the drained queue
is split into **partitions**, one per distinct transport envelope (headers,
underlying HTTP client/session, codec/serializer, destination URL). Every
call within a partition is guaranteed to produce byte-identical wire
behavior if sent alone, so merging them changes nothing observable about
any individual call except its timing. Calls with different auth headers,
for instance, land in different partitions and become different wire
requests — never one request with one header set silently applied to all of
them (this was a real historical bug, #273: see §2.3 note below).

Reference: `packages/cratestack-link-batch/src/signature.ts`
(`effectiveConfig` + `batchSignature` + `partition`) — the signature is a
tuple of (client-instance identity, codec identity, resolved batch URL,
sorted header list minus any per-frame-carried header like an idempotency
key). Two calls partition together iff their tuples are equal.

**Historical note (#273)**: before this partitioning existed, the flush
reused the *first* queued call's envelope for the entire batch, silently
dropping later calls' distinct headers. The fix — computing a signature per
call and grouping by it — is part of the model, not an implementation
afterthought; a port that skips partitioning reintroduces that bug.

### 2.4 Dedup — explicit-key only

Within a partition, calls may additionally collapse into **one wire frame**
if they share a dedup key. The default dedup function returns a key **only
when the call carries an explicit idempotency key**, and returns "never
collapse" (a null/none key) otherwise. This is a safety rule, not a
performance default: the transport has no way to know two unmarked calls
are safe to treat as the same operation, so guessing (e.g. hashing the
call's arguments) risks silently merging two textually-identical but
semantically distinct mutations. Only the caller's own explicit signal
authorizes collapsing.

Reference: `index.ts` line 9 (`defaultDedupe`), `types.ts` lines 17–29 (docs
on the option), `correlate.ts` lines 7–28 (`groupByDedupeKey`). Dedup is
pluggable — a caller may supply a fully custom key function, or `() => null`
to disable it and keep only the "one wire request" behavior — but the
*default* is the safety rule above, and #181's AC ("idempotency-keyed and
streaming calls are never silently coalesced in a way that changes their
semantics") is enforced by that default, not by an opt-in the caller might
forget.

### 2.5 Dispatch + per-frame correlation

Each partition (after dedup grouping) is sent as a single request whose body
is an ordered sequence of frames, one per (deduped) group, each carrying a
caller-chosen `id`. The wire contract guarantees the response is an ordered
sequence of the same length (`docs/design/rpc-transport.md` §3.2), but
correlation is done **by `id`, not array position** — strictly more robust,
and consistent with the repo's Rust-side batch debouncer
(`examples/rpc-batch-debounce`). Each response frame is then handed back to
every pending result handle in its group, independently.

Reference: `dispatch.ts` (`dispatchPartition`) builds the frame sequence and
calls `resolveGroups`; `correlate.ts` lines 31–68 (`resolveGroups`) maps
`frames` by `id` and settles each queued caller from its own frame. Note
line 61–65: the TS implementation re-encodes each frame with **the calling
runtime's own codec**, not the batch's — a caller must decode exactly what
it would have decoded on the unary path, regardless of which codec the
aggregate wire request used. This is model-relevant, not an implementation
detail: whatever language a port uses, "the result you construct for a
caller must be indistinguishable from what a non-batched call would have
produced" is part of the contract, because it's what lets every existing
caller (including generated data-fetching hooks) benefit from batching with
zero code changes.

### 2.6 Error isolation

Two independent failure axes, both scoped as narrowly as possible:

- **Per-frame**: one op failing inside a successfully-transported batch
  response settles only the pending result handle(s) in that frame's group;
  siblings in the same wire response settle from their own (successful)
  frame.
- **Per-partition (whole-request failure)**: if the wire request for a
  partition fails outright (network error, non-2xx with unparseable body),
  only the pending result handles queued in *that partition* are rejected —
  a concurrently in-flight partition (different envelope) is unaffected.

Reference: `dispatch.ts` lines 25–54 (`try`/`catch` scoped to one partition's
`dispatchPartition` call) and `correlate.ts` lines 38–46 (missing frame →
reject only that group).

### 2.7 Cancellation (best-effort, documented limitation)

If a caller has a cancellation signal for the underlying call, cancelling it
before flush removes that call from the queue and settles it as cancelled
without affecting siblings. Cancelling after flush is a no-op — there is no
network-level cancellation of an already-in-flight aggregate request, since
canceling the HTTP request would abort every other call sharing that wire
request too. This is a known, accepted limitation, not a gap to close in a
port.

Reference: `index.ts` lines 79–93 (`AbortSignal` listener that only acts if
the entry is still in `queue`).

## 3. Evaluation against Dio's `QueuedInterceptor`/interceptor+queue model

This is the check #181 and epic #207 explicitly ask for: does §2's model map
onto Dio without redesigning it, or does it turn out to be secretly
`fetch`/`Promise`-shaped after all?

### 3.1 What `QueuedInterceptor` actually is — and isn't

`QueuedInterceptor` (Dio's `dio` package) is a specialized `Interceptor`
that serializes requests behind an explicit `lock()`/`unlock()` gate: while
locked, every request hitting the interceptor queues up in arrival order;
`unlock()` releases them to proceed **one at a time, each as its own
independent wire request**, in the order they queued. Its canonical use
case is auth-token refresh — lock while a refresh call is in flight, queue
everything that arrives meanwhile, unlock once the new token is available so
the queued requests retry with it.

That is a **request-ordering/pausing** primitive, not a **request-merging**
primitive. `QueuedInterceptor` never combines N logical requests into one
wire request the way §2's model does — it defers and then replays them
individually. Naively "porting onto `QueuedInterceptor`" would not produce
batching at all; it's the wrong building block, despite being the
Dio class whose name looks most relevant.

### 3.2 The primitive that actually maps: a deferred, self-resolving handler

The piece of Dio that §2's model actually needs is one layer down: a plain
`Interceptor.onRequest(RequestOptions options, RequestInterceptorHandler
handler)` that does **not** call `handler.next(options)` synchronously.
Dio explicitly supports holding onto `handler` and calling
`handler.resolve(response)` (or `.reject(error)`) from arbitrary later async
code — this is the same mechanism `QueuedInterceptor` itself is built on
top of, and it is also Dio's documented pattern for "inject something
asynchronous before this request proceeds" (e.g. awaiting a cached token).

That handler-hold-then-resolve/reject shape is structurally identical to
the TS model's `new Promise((resolve, reject) => { queue.push({request,
resolve, reject}); ... })` (§2.1): a JS `Promise` executor's `resolve`/
`reject` and a Dio `RequestInterceptorHandler`'s `.resolve()`/`.reject()`
are the same primitive — "the pending call is not settled yet, and
something outside the normal control flow will settle it later." A plain
custom `Interceptor` (not `QueuedInterceptor`) holding a queue of
`(options, handler)` pairs is the direct Dio equivalent of §2.1.

### 3.3 Piece-by-piece mapping

| §2 behavior | TS (`@cratestack/link-batch`) | Dio port |
|---|---|---|
| 2.1 Queue accumulation | `queue.push({request, resolve, reject})` inside a `Promise` executor | Custom `Interceptor.onRequest`: push `{options, handler}` onto a `List`, do **not** call `handler.next()` |
| 2.2 Flush trigger | `queueMicrotask` (default) or `setTimeout(run, windowMs)` | `scheduleMicrotask` (`dart:async`, default) or `Future.delayed(Duration(milliseconds: windowMs), run)` — Dart's microtask queue drains between synchronous segments exactly as JS's does, so `Future.wait([...])` (Dart's `Promise.all`) exhibits the same same-tick coalescing |
| 2.3 Partitioning by envelope | `WeakMap<object, id>`-based reference identity for `fetch`/codec + sorted header list, joined into a signature string | `Expando<int>` (Dart's identity-keyed ephemeral map — the direct `WeakMap` analog; unlike `WeakMap`, `Expando` also forbids non-object keys, which is fine since the keyed values here are already objects: the `Dio` instance and the codec) for `Dio` instance / codec identity, plus `options.headers` — same signature-string construction |
| 2.4 Dedup, explicit-key only | `request.idempotencyKey !== undefined ? "idem:" + key : null` | Same rule against whatever field carries the idempotency key in `options.extra` or a typed request wrapper — the *rule* ("only an explicit key licenses collapsing") is language-independent, not a JS-specific default |
| 2.5 Dispatch + correlation | One `fetch(batchUrl, {...})`, decode frames, map by `frame.id` via a `Map` | One `dio.post(batchUrl, data: frames)` (issued directly on the `Dio` instance, bypassing this interceptor to avoid re-queueing itself — same as the TS code calling raw `fetch` instead of going through the link chain again), decode frames, map by `frame.id` via a `Map<int, Frame>` |
| 2.5 Per-caller re-encode with own codec | `new Response(entry.request.codec.encode(body), {status})`, so downstream decode is codec-agnostic | Construct a `Response<T>` directly (Dio allows building arbitrary `Response` objects) carrying the correlated, re-encoded body and the original `RequestOptions`, then `handler.resolve(response)` |
| 2.6 Per-frame error isolation | `entry.reject(error)` for the one caller only | `handler.reject(DioException(requestOptions: ..., error: ...))` for the one held handler only |
| 2.6 Per-partition failure isolation | `try`/`catch` scoped to one `dispatchPartition` call | `try`/`catch` scoped to one partition's dispatch coroutine — Dio's own request/response cycle already throws `DioException` per call, so the isolation boundary is the same `try`/`catch`-per-partition shape |
| 2.7 Cancellation, best-effort | `AbortSignal` listener, no-ops once flushed | `CancelToken` listener (`cancelToken.whenCancel`), same no-op-once-flushed limitation — Dio's own docs make the same "cancelling doesn't abort an in-flight request that already left the queue" caveat for its built-in transformers |

### 3.4 Confirmed portable — no redesign required

Every row in §3.3 has a direct Dio counterpart using **public, documented**
Dio APIs — none of it requires Dio internals, forking Dio, or abandoning
`QueuedInterceptor`-adjacent idioms Dart developers already recognize. The
one correction versus a naive reading of #181/#207 ("port onto
`QueuedInterceptor`") is §3.1/§3.2: the mapping target is a plain
`Interceptor` with a deferred handler, not `QueuedInterceptor` itself. That
is a clarification of *which* Dio primitive to use, not evidence the model
needs to change shape — the model in §2 was written before this evaluation
and required no edits after checking it against Dio's actual API surface.

**Conclusion for #181's AC**: "the batching/coalescing model is written up
independently of TypeScript so it can be evaluated for a Dart/Flutter port
without a redesign" — confirmed. §2 is that write-up, stated without TS
vocabulary; §3 is the evaluation, and it found a straightforward mapping,
not a blocker.

### 3.5 Idiomatic substitutions worth flagging up front for the future port

These are naming/API differences a Dart implementer should expect, not
open design questions:

- `WeakMap` → `Expando` (§3.3, 2.3).
- `queueMicrotask`/`setTimeout` → `scheduleMicrotask`/`Future.delayed`
  (§3.3, 2.2).
- JS `Promise` executor `resolve`/`reject` → Dio's
  `RequestInterceptorHandler.resolve`/`.reject` (§3.2).
- `AbortSignal` → `CancelToken` (§3.3, 2.7).
- The TS link is installed into an `RpcLink` chain (#182); the Dio
  equivalent is installed into `Dio.interceptors`. Both are ordered,
  composable middleware lists, so "batching composes with a logger/retry/
  auth-refresh interceptor instead of clobbering them" (the TS package's own
  stated design goal) holds in Dio the same way.

## 4. Non-goals (mirrors #181's own Out of Scope)

- Implementing the Dart/Flutter port itself — tracked as a future ticket
  once scheduled, per #181 and epic #207 ("Any Dart/Flutter implementation
  work itself... a Dart port is a follow-up once that model exists").
- Server-side batch changes — `POST /rpc/batch` (`docs/design/rpc-transport.md`
  §3.2) is unchanged and is not evaluated here beyond citing its contract.
- Cross-frame transactional/in-batch-dependency batching — rejected by
  design at the server level (§3.2's "encoding workflow into the wire
  protocol is rejected by design"); nothing in §2/§3 revisits that.

## 5. Follow-up

- When a Dart/Flutter RPC client generator is scheduled, this document
  (§2 for the model, §3.3 for the piece-by-piece mapping) is the intended
  starting point — no new design spike should be needed first.
- If that future work finds a row in §3.3 doesn't hold up against Dio's
  actual behavior in practice (as opposed to its documented API), that's a
  correction to make *here*, not a reason to silently diverge the Dart
  implementation from the model.
