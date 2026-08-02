# Streaming RPC links — spike

Status: **spike** (2026-08-02) — no production code changes. See [issue #274](https://github.com/cratestack/cratestack/issues/274).
Scope: `packages/cratestack-ts-types`'s `RpcLink` contract and the generated
`crates/cratestack-client-typescript/templates/src/rpc-*.ts.j2` templates, for `transport rpc` schemas.

## 1. The problem

`CratestackRpcRuntime.stream()` bypasses the `links` chain entirely (issue
[#182](https://github.com/cratestack/cratestack/issues/182)):

```ts
// crates/cratestack-client-typescript/templates/src/rpc-runtime.ts.j2
async *stream<O>(opId: string, input: unknown, options: CratestackRpcCallOptions = {}): AsyncIterable<O> {
  // ... calls this.fetchFn(...) directly. this.chain is never touched.
}
```

The stated reason, at the point of exclusion, was that a link wanting to inspect or retry a
response would need to clone a streamed body, defeating streaming. That reasoning holds for the
*existing* `Response`-shaped contract (`RpcLink = (req, next) => Promise<{ response: Response }>`)
— it says nothing about whether a *frame-shaped* contract could work. This spike evaluates that.

**Consequence today:** `createLoggerLink()`, `createBatchLink()`, `@cratestack/validator-zod`, and
any user-authored retry/auth-refresh link are all silent no-ops for streaming calls. A user who
wires up logging and then calls a streaming procedure gets no logs and no indication why not.

## 2. The tempting wrong answer

The intuitive fix is to unify everything under one generator-shaped contract:

```ts
export function createMyLink(): AsyncGenerator<RpcLink, TReturn, TNext> {
  return async function* (request, next) {
    yield next(request);
  };
}
```

This does not typecheck as written (`AsyncGenerator<RpcLink, ...>` yields `RpcLink` values, not
response frames — a link *is* the generator, it doesn't return one), but the underlying instinct —
"generators are how you compose streams" — is directionally right for `stream()` and directionally
**wrong** for `call()`/`batch()`. §3 has the numbers.

## 3. Benchmark: async-function chain vs async-generator chain

Harness: [`bench.mjs`](./streaming-rpc-links/bench.mjs) (plain Node, no dependencies, run it
yourself — `node docs/design/streaming-rpc-links/bench.mjs`). Depth 4 (the ticket's "realistic
depth, 3-5 links"), 200k iterations, warmed up before measuring.

```
depth=4 iterations=200000 node=v24.14.1

-- chain overhead in isolation (no network) --
async-function chain                   total 62.5ms  per-call 0.312us
async-generator chain                  total 292.8ms  per-call 1.464us

generator overhead vs async-function: 368.4%

-- chain overhead as a fraction of a realistic call (+5ms network) --
async-function chain + network         total 11156.0ms  per-call 5578.013us
async-generator chain + network        total 11192.7ms  per-call 5596.362us

generator overhead vs async-function, with network: 0.329%
```

**Reading this honestly** (this is a correction of an overstated claim made in chat before this
spike ran — worth being precise here rather than just confirming the prior): a blanket
generator-based unification is a *real* per-call regression (368%, i.e. ~4.7x), but the *absolute*
cost is ~1.15 microseconds. Once a call does real I/O — even a fast 5ms same-region round trip —
that difference is 0.33% of the call and unmeasurable in practice. **The perf argument alone does
not decide this** for the common case of a call that hits the network.

Where it *does* matter: calls that resolve **without** any network I/O.

- `createBatchLink`'s dedup path — a call that shares a dedupe key with an already-in-flight one
  resolves from the batch response the *other* call triggers, with no `fetch` of its own.
- `@cratestack/validator-zod`/`-yup` short-circuiting on a validation failure — rejects before
  `next()` is ever called.
- Any future cache-read link.

For these, chain overhead **is** the visible cost, and 368% is real (though still, at ~1.15us,
below the threshold most applications would notice — this is a "don't make it worse for free"
argument, not a "this is currently a problem" one).

## 4. Recommendation: two chains, not one

```ts
// Unchanged — this is #182's existing contract, untouched.
export type RpcLink = (request: RpcLinkRequest, next: RpcLinkNext) => Promise<RpcLinkResponse>;

// New — only stream() runs through this one.
export interface RpcStreamFrame<O = unknown> {
  readonly output: O;
}
export type RpcStreamLinkNext = (request: RpcLinkRequest) => AsyncIterable<RpcStreamFrame>;
export type RpcStreamLink = (
  request: RpcLinkRequest,
  next: RpcStreamLinkNext,
) => AsyncIterable<RpcStreamFrame>;
```

`stream()` builds its chain the same way `call()`/`batch()` already do (`reduceRight` wrapping a
terminal), just with `RpcStreamLink` and an async-generator terminal instead of a promise-returning
one:

```ts
const terminalStreamLink: RpcStreamLinkNext = async function* (request) {
  // ... same fetchFn call as today's stream(), decode each frame, yield it.
};
this.streamChain = (options.streamLinks ?? []).reduceRight<RpcStreamLinkNext>(
  (next, link) => (request) => link(request, next),
  terminalStreamLink,
);
```

A link implementer who wants to log/retry a stream now can, by consuming and re-yielding:

```ts
export function createLoggerStreamLink(logger = console): RpcStreamLink {
  return async function* (request, next) {
    logger.info(`[rpc] -> stream ${request.opId}`);
    let count = 0;
    for await (const frame of next(request)) {
      count++;
      yield frame;
    }
    logger.info(`[rpc] <- stream ${request.opId} (${count} frames)`);
  };
}
```

No body-cloning problem: each link consumes the async iterable it's handed and yields its own
frames onward — there's no `Response` object to clone, because there's no single `Response` in the
frame-shaped contract at all.

### Why not unify (Apollo vs. tRPC precedent)

Apollo Link unifies everything under `Observable` — a single contract for query/mutation/
subscription. tRPC v11 keeps a promise-based unary/batch link contract and uses async generators
*only* for its streaming/subscription links — the same two-contract split recommended here. Given
§3's numbers don't force either choice, this follows tRPC's precedent rather than Apollo's, for a
reason specific to this codebase: **Dart/Rust client portability**. This repo already has a
standing convention — established when issue #181's batching design was redirected away from a
TypeScript-only, Promise/microtask-shaped model toward one framed against multiple ecosystems'
idioms (Dio's interceptor/queue model for Dart, TanStack Query's dedup for TS) up front — that new
cross-cutting client-runtime abstractions (`cratestack-client-{rust,typescript,dart,flutter}`) must
be designed language-agnostic, not ported wholesale from whichever framework is top-of-mind. A JS
async generator has no clean Dart equivalent to port — Dart's own idiom is `Stream<T>`, which maps
far more directly onto an **Observable/Stream-shaped** contract than onto `AsyncGenerator`. Keeping
the unary contract promise-based (already portable — every language has "a function returning a
future") and only introducing a stream-shaped contract for the one thing that's actually a stream
keeps the *port surface* minimal instead of making the common contract harder to port for a perf
number that doesn't hold up at the margin that matters.

## 5. Migration shape

- Every existing `RpcLink` implementation (`@cratestack/link-batch`, `@cratestack/link-logger`,
  `@cratestack/validator-zod`, `@cratestack/validator-yup`, and any user-authored one) is
  **unchanged** — `RpcLink` itself doesn't change shape, only a new `RpcStreamLink` is added
  alongside it.
- `CratestackRpcClientOptions` gains a new `streamLinks?: RpcStreamLink[]` field, separate from
  `links?: RpcLink[]` — a project that never streams never needs to touch it, matching the
  "omitted is a true no-op" property `links` already has.
- Codegen (`rpc-runtime.ts.j2`, `rpc-links.ts.j2`) needs the new type + `stream()`'s chain
  construction; `packages/cratestack-ts-types` needs the pinned copy kept in sync, same as today's
  `RpcLink`.
- **Blocked on resolving the existing CBOR-seq TODO first.** `stream()` currently throws
  `CratestackRpcTransportError` for `application/cbor-seq` responses (no decoder wired — see the
  `// TODO: wire a CBOR-seq decoder` comment in `rpc-runtime.ts.j2`). A frame-based link contract
  needs real frames to test against; implementing it against only the JSON-array-body streaming
  path (the one case that already works) would under-test the contract against what streaming
  usually actually looks like in production (CBOR default codec). This makes the follow-up ticket
  larger than "add a type and a chain," and it should say so explicitly.

## 6. Out of scope (confirmed unchanged by this spike)

- REST and gRPC-Web streaming parity — RPC-only, per #182's own original scope.
- The `@@subscribe` transport question (SSE vs. WS) — that's #183, a sibling ticket under the same
  epic (#207), answering a different question (which wire protocol) than this one (which client
  contract).
- Any change to today's `RpcLink` for `call()`/`batch()`.

## 7. Follow-up

Filed as [#277](https://github.com/cratestack/cratestack/issues/277): implement `RpcStreamLink` per
§4-5 above. Depends on resolving the CBOR-seq decoder TODO first (§5).
