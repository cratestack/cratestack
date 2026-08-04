# RPC transport — v1 design

Status: **accepted** (2026-05-15) — HTTP surface **shipped** in PRs #20–#24.
Scope: schemas declaring `transport rpc` in `.cstack`.

## Shipped vs. pending

| Item | Status | Where |
|------|--------|-------|
| `transport rpc` directive + `OpDescriptor` vocabulary | shipped | #20 |
| Unary runtime for procedures + `cratestack-axum::rpc` primitives | shipped | #21 |
| CRUD over RPC unary + `POST /rpc/batch` | shipped | #22 |
| `RpcErrorBody` with gRPC-style codes (uniform on every error path) | shipped | #23 |
| Streaming wire format for `Sequence`-kind ops via `Accept: application/cbor-seq` | shipped (content negotiation + framing) | #24, corrected #281 |
| `@stream` schema directive + stream-shaped `ProcedureRegistry` trait method | shipped | #282 |
| Genuinely incremental delivery for `@stream` ops (`Body::from_stream`, mid-stream error sentinel, client-disconnect cancellation) | shipped | #283 |
| SSE subscription binding (§3.4a, `@@subscribe` schema directive) — recommended first path | shipped | #183, #390 |
| WebSocket binding + subscriptions (§3.4) | **pending**, gated on a real bidirectional/multiplexing case | — |
| Batch parallelization | deferred (no observed contention) | — |

Streaming now delivers incrementally, but only for ops explicitly marked
`@stream` (#282) — a plain `T[]`-returning procedure still gets
`OpKind::Sequence` at the wire-descriptor level (content negotiation,
`application/cbor-seq` framing) but keeps the original buffered
behavior: `encode_cbor_sequence_response`
(`crates/cratestack-axum/src/transport/internal.rs`) still builds the
entire `Vec<u8>` before constructing the one `axum::Response`, unchanged
by #283 by design (see that ticket's non-breaking acceptance criterion).

For `@stream` ops, `crates/cratestack-axum/src/transport/
stream_sequence.rs` (`encode_transport_stream_result_with_status_for` in
`encode_sequence.rs` is the entry point) builds an `axum::body::Body::
from_stream` response instead: each item is encoded and flushed onto the
wire as it's produced, with no `Vec<u8>` ever fully materialized
server-side. A mid-stream failure is signaled per §3.3 (the tag-48900
CBOR sentinel) as the final item, never a status-code change (the
response has already committed to 200 by the time any body byte can be
written). Client disconnect mid-stream drops the underlying item
producer promptly — proven by a real TCP-level integration test
(`examples/rpc-streaming/tests/stream_disconnect.rs`), not just asserted
in a docstring. `IdempotencyService` detects a genuinely streamed
response (an internal marker header, not content-type sniffing — see
`crates/cratestack-axum/src/idempotency/stream_bypass.rs`) and bypasses
buffering/replay for it entirely rather than silently re-collecting a
partial stream, since the handler has already run by the time that
decision point is reached and idempotency-replaying a partial stream has
no defined semantics.

Non-cbor-seq negotiated responses to a `@stream` op (e.g. a plain JSON
`Accept`) still fall back to draining the stream into a `Vec` and
reusing the buffered encoder — arrays can't be flushed incrementally the
same way, and this section's incremental-delivery guarantee is scoped to
`application/cbor-seq`, matching §3.3.

Subscriptions now ship over SSE (`@@subscribe`, §3.4a, cratestack#390):
`GET /rpc/subscribe/{op_id}` streams `ModelEvent<T>` items over the
existing `CoolEventBus`, reusing the `@stream` encoder's `Stream ->
axum::body::Body::from_stream` shape with SSE framing instead of
cbor-seq. Delivery rides the same outbox-drain mechanism `@@emit` has
always used (`crates/cratestack-sqlx/src/descriptor.rs::
drain_event_outbox`, invoked automatically once a mutating op's
transaction commits) — no new delivery pipeline. Backpressure is a
bounded per-subscription channel (`crates/cratestack-axum/src/rpc/
subscription_bridge.rs`); on overflow the channel closes, which the
encoder (`crates/cratestack-axum/src/rpc/sse.rs`) turns into a terminal
`Error{code:"unavailable"}` SSE event, ending the stream — the client
decides whether to resubscribe (fire-and-forget, no cursors, matching
§3.4). Cleanup on disconnect or overflow uses a new
`cratestack_core::SubscriptionGuard`/`CoolEventBus::unsubscribe`, so a
long-running server doesn't accumulate one permanently-registered
handler per historical connection. The WS binding (§3.4) remains the
only HTTP-surface gap, still gated on a concrete bidirectional/
multiplexing case — see §6.

The REST binding is and remains the default. RPC is an alternative *generation
style* — a schema picks one or the other via the `transport` directive, and
the macro emits exactly one binding's worth of routes, descriptors, and
client surface. There is no runtime flip between them.

## 1. Why a second binding at all

The REST binding maps each model verb and each `@procedure` to a unique HTTP
route. That is good for cacheability, CDN behavior, per-route observability,
and the broad tooling ecosystem that already understands HTTP verbs and
status codes. It is bad for:

- Batching N operations into one roundtrip.
- Streaming a sequence of values from one logical call.
- Subscribing to model events as a first-class call.

The RPC binding addresses those three. It does **not** try to be a better
REST. Schemas that don't need batching/streaming/subscriptions should stay
on `transport rest`.

## 2. Vocabulary

### 2.1 Op identity

Every callable in a `transport rpc` schema gets a stable string id. The id
is the only dispatch key.

| Schema construct                            | Op id                       | Kind           |
| ------------------------------------------- | --------------------------- | -------------- |
| `model User { ... }`                        | `model.User.list`           | `Unary`        |
| `model User { ... }`                        | `model.User.get`            | `Unary`        |
| `model User { ... }`                        | `model.User.create`         | `Unary`        |
| `model User { ... }`                        | `model.User.update`         | `Unary`        |
| `model User { ... }`                        | `model.User.delete`         | `Unary`        |
| `model User { ... } @@subscribe(...)`       | `model.User.subscribe`      | `Subscription` |
| `procedure foo(...)`                        | `procedure.foo`             | `Unary`        |
| `mutation procedure foo(...)`               | `procedure.foo`             | `Unary`        |
| `procedure foo(...) @stream`                | `procedure.foo`             | `Sequence`     |

The mutation-ness of a procedure is metadata on the descriptor, not part of
the id. The `@stream` and `@@subscribe` syntaxes do not exist yet — they
will be added together with the macro emitter for this binding.

### 2.2 Op descriptor

The macro emits, for each callable, a `const` of:

```rust
pub struct OpDescriptor {
    pub op_id: &'static str,
    pub kind: OpKind,
    pub input_ty: &'static str,
    pub output_ty: &'static str,
    pub idempotent_by_default: bool,
    pub auth_required: bool,
}

pub enum OpKind { Unary, Sequence, Subscription }
```

This lives alongside (not in place of) `RouteTransportDescriptor`. REST
schemas emit `RouteTransportDescriptor`s; RPC schemas emit `OpDescriptor`s.
A single schema does not emit both.

### 2.3 Frame envelope

Frames are codec-encoded (CBOR or JSON, whichever the binding negotiated).
One schema, six variants:

```text
Request    { id: u64, op: string, in: <codec value>, idem?: string }
Response   { id: u64, out: <codec value> }
StreamItem { id: u64, next: <codec value> }
StreamEnd  { id: u64, end: {} }
Cancel     { id: u64, cancel: {} }
Error      { id: u64, err: ErrorBody }
```

`id` is client-chosen and unique within a connection (or batch). `op` is
omitted on response frames — they correlate via `id`. The variant is
discriminated by which of `out` / `next` / `end` / `cancel` / `err` is
present, not by a separate `type` tag.

```text
ErrorBody {
    code:    string,
    message: string,
    details?: <codec value>,
}
```

Stable `code` values, modeled on gRPC: `not_found`, `invalid_argument`,
`permission_denied`, `failed_precondition`, `conflict`, `unauthenticated`,
`internal`, `unavailable`, `deadline_exceeded`, `canceled`. Each maps to an
HTTP status for the unary HTTP binding.

## 3. Bindings

The RPC generation style emits four bindings; clients pick whichever fits
the call site. All four share one op registry, one codec, one policy
pipeline, one idempotency store, one audit sink.

### 3.1 HTTP unary — `POST /rpc/:op_id`

The op id appears in the URL, not the body. This is deliberate:

- nginx, CDNs, and HTTP tracing tools work per-route without parsing
  payloads.
- `curl http://.../rpc/model.User.list -d '...'` is a debuggable artifact
  in tickets and runbooks.
- Per-op metrics fall out of standard HTTP middleware.

On the wire the frame is *unwrapped*:

- Request body = the `in` payload, codec-encoded directly. No `{id, op, in}`
  wrapper.
- Response body on success = the `out` payload, codec-encoded.
- Response body on error = `ErrorBody`, codec-encoded. HTTP status mapped
  from `code`.
- `Idempotency-Key` header reuses `cratestack-axum::idempotency` unchanged.
- `Authorization` header same as today.
- `Content-Type` / `Accept` negotiate codec the same way the REST binding
  does today via `validate_codec_request_headers`.

### 3.2 HTTP batch — `POST /rpc/batch`

The frame is wrapped here because the wire carries N requests.

- Request body = codec-encoded sequence of `Request` frames.
- Response body = codec-encoded sequence of `Response | Error` frames,
  **same order as the request sequence** so order-only clients can zip
  without an `id` lookup.
- HTTP status: 200 if the batch parsed, regardless of per-frame outcomes.
  400 only on codec-malformed batches.
- Per-frame idempotency: optional `idem` field on each `Request`. The
  `Idempotency-Key` header is rejected on this route as ambiguous.
- **Not transactional.** Each frame runs in its own transaction. The server
  is free to fan frames out in parallel.
- **No in-batch dependencies.** A batch like
  `[create A, update B referencing A.id]` is not supported. The correct
  shapes are (a) two roundtrips, or (b) a single `@procedure` that owns the
  composite operation. Encoding workflow into the wire protocol is rejected
  by design — it is how RPC frameworks rot.

### 3.3 HTTP server-streamed — `POST /rpc/:op_id`, negotiated

For ops where `kind == Sequence` (today: `procedure foo @stream`; in the
future, `model.User.list @stream`).

- Client sends `Accept: application/cbor-seq` (already encoded by
  `encode_cbor_sequence_response`) or `text/event-stream` for SSE.
- Each chunk is *one* unwrapped `out` payload — no frame wrapper, no `id`.
  End of stream is end of body.
- **Mid-stream errors on `application/cbor-seq`: an in-band CBOR tag, not a
  second content-type and not an HTTP trailer.** An earlier version of this
  section described the error signal as "a trailing chunk with content-type
  `application/cratestack.error+cbor`," which is not physically realizable:
  an HTTP response has exactly one `Content-Type` header, set once, before
  any body bytes — there is no such thing as a second content-type
  appearing mid-body. The other plausible reading, an HTTP trailer, does not
  work either: the Fetch API spec does not expose HTTP trailers to
  JavaScript in any browser, and this framework's TypeScript client
  streaming (`stream()`) is built on browser `fetch()`. The corrected,
  implementable mechanism:

  - When the underlying operation ultimately fails, the *last* item in the
    `application/cbor-seq` sequence is `Tag(48900, RpcErrorBody-as-CBOR-map)`
    — CBOR major type 6 (tag), tag number **48900**, wrapping the
    `RpcErrorBody` (§2.3) encoded as a plain CBOR map, in place of what
    would otherwise be the next unwrapped `out` item.
  - Tag 48900 is reserved exclusively for this purpose: it never wraps a
    successful `out` value, and it is only ever the *last* item of a
    sequence. A stream that emits a tag-48900 item never resumes normal
    output afterward — end of body follows immediately after it.
  - A boundary-scanner walking the byte stream (see
    `CborSeqChunkDecoder` in `cratestack-client-rust/src/streaming.rs`, and
    its planned TypeScript port) detects this *structurally*: once its
    existing `minicbor`-based boundary detection lands on a complete
    top-level item, checking whether that item's leading byte(s) decode to
    major type 6 with tag number 48900 classifies it as an error envelope
    versus a normal output item — the scanner does not need to fully decode
    the item's payload to make that call, only recognize the tag header.
  - **Tag number provenance:** 48900 was picked from IANA's CBOR tags
    registry "First Come First Served" range (32768 –
    18446744073709551615; see
    <https://www.iana.org/assignments/cbor-tags/cbor-tags.xhtml>) and was
    unassigned as of 2026-08-02, the date this section was corrected. It is
    **not** formally registered with IANA — the PR that introduced this
    correction (source: issue #281) documents the verification method used
    and flags this number for a pre-merge human double-check, per the
    collision risk noted in that ticket.
  - SSE framing (`text/event-stream`) is unaffected by this correction: SSE
    natively supports multiple named event types within a single response,
    so `event: error` was always a physically realizable signal there. This
    fix only concerns the `application/cbor-seq` encoding, where a second
    content-type or a trailer were the (unrealizable) options originally
    described.
- **~~No subscriptions over HTTP streaming~~ — revised, see §3.4a.** This
  objection was written for arbitrary/general streaming; issue #183's spike
  found it doesn't transfer to `@@subscribe`'s actual fire-and-forget,
  one-subscription-per-connection model. SSE is now a first-class
  subscription binding alongside WS — see §3.4a.

### 3.4 WebSocket — `GET /rpc/ws` upgrade

- Subprotocol offers: `cratestack.rpc.v1+cbor`, `cratestack.rpc.v1+json`.
  Server picks one. WS close 1002 if none acceptable.
- Authentication: the upgrade request is HMAC-signed via the existing
  `HmacEnvelope` mechanism. Frames inside the established session are
  **not** individually signed — the channel is authenticated at upgrade.
  Re-keying / session expiry: server emits
  `Error { code: "unauthenticated" }` on affected ids and closes; client
  reconnects.
- One frame per WS message. Binary frames carry CBOR, text frames carry
  JSON.
- All six frame variants from §2.3 are used.
- Subscriptions: `Request { op: "model.User.subscribe", in: { filter } }`
  → server emits `StreamItem { next: ModelEvent<User> }` over the
  `CoolEventBus` until the client sends `Cancel { id }` or the connection
  drops.
- Subscriptions are **fire-and-forget**. No cursors, no replay buffer. A
  client that misses events while disconnected has missed them. Server-to-
  server callers do not need replay; external clients resubscribe on
  reconnect.
- Backpressure: bounded per-subscription send buffer; on overflow the
  server emits `Error { code: "unavailable", message: "subscription
  lagged" }` and ends the stream. The client decides whether to
  resubscribe.

### 3.4a SSE — `GET /rpc/subscribe/{op_id}` (decision record, issue #183)

**Spike decision (2026-08-04): amend, don't replace.** SSE is a first-class
subscription binding alongside WS for the fire-and-forget, no-replay model
§3.4 already commits to — reusing the `application/cbor-seq`/SSE machinery
§3.3 already ships, rather than requiring the full WS frame loop up front.
WS stays in this design for a different, not-yet-stated future need (true
bidirectional communication, or many subscriptions multiplexed over one
connection at scale) — build that only when a concrete case for it shows
up, the same way this spike itself was triggered by a concrete question
rather than speced in advance.

Why the §3.3 objection ("SSE cancellation races with backpressure") doesn't
transfer to `@@subscribe` specifically:

- **Cancellation.** §3.4 already treats "the connection drops" as an
  equally valid cancellation path alongside the explicit `Cancel{id}`
  frame. `@@subscribe` is per-op — one subscription per connection, not N
  multiplexed ops sharing a socket — so there is no `id` to disambiguate in
  the first place. WS's `Cancel{id}` solves a multiplexing problem this
  model doesn't have.
- **Backpressure.** §3.4's own overflow handling is already
  server-unilateral (bounded buffer → emit `Error` → end stream), not
  `Cancel`-dependent — identical behavior over SSE (`event: error`, already
  a physically realizable signal per §3.3's own cbor-seq tag-48900
  correction) or WS. The "race" in §3.3 is generic TCP-teardown detection,
  not something SSE handles worse than WS for this model.
- **Reconnect/resume.** §3.4 already commits to no cursors, no replay — a
  disconnected client just resubscribes fresh. SSE needs zero new semantics
  under that rule, and the browser `EventSource` API auto-reconnects with
  backoff natively, which a WS client has to hand-roll.
- **The one real cost:** no multiplexing — a client subscribing to several
  models needs one SSE connection per subscription. Under HTTP/2 (this
  design's assumed deployment target throughout) this is a non-issue;
  connections are multiplexed at the transport layer. This only becomes a
  real constraint for HTTP/1.1-only deployments, which nothing else in this
  document assumes.

Wire shape: `GET /rpc/subscribe/{op_id}` (e.g.
`/rpc/subscribe/model.User.subscribe`), `Accept: text/event-stream`, same
auth as the existing HTTP bindings (header, not upgrade-time HMAC — SSE has
no upgrade handshake to sign). Each `StreamItem`/`Error` from §2.3 becomes
one SSE event, reusing the encoder path already proven by streaming
(§3.3). No new frame types, no new envelope.

Implemented in cratestack#390: `@@subscribe` (a bare model-level
attribute, mirroring `@@audit`/`@@soft_delete`; requires `@@emit(...)`
on the same model and `transport rpc` on the schema — enforced at parse
time) emits the `OpKind::Subscription` `model.<X>.subscribe` op
descriptor, and `GET /rpc/subscribe/{op_id}` dispatches it — header-
based auth via the existing `AuthProvider`, then one `CoolEventBus`
registration per `@@emit`ted operation, bridged through a bounded
channel into the SSE encoder. §3.4's WS design stays written down as-is
for whenever a real bidirectional or high-multiplexing need
materializes; it isn't wrong, it's just not what the case in front of
us needs today.

Row-level `@@allow` policy is **not** replayed against streamed
events — that machinery lives in the SQL query builders and has no
analogue for an in-memory outbox-sourced event. A subscription client
only needs to authenticate; it does not get per-row filtering the way
`list`/`get` do. This is a deliberate, documented scope limit for the
first cut, not an oversight — revisit if a concrete case needs it.

## 4. Cross-binding concerns

| Concern              | HTTP unary    | HTTP batch        | HTTP stream         | WS                       |
| -------------------- | ------------- | ----------------- | ------------------- | ------------------------ |
| Auth                 | header        | header            | header              | upgrade-time HMAC        |
| Idempotency key      | header        | per-frame field   | header              | per-frame field          |
| Cancellation         | close conn    | n/a (whole batch) | close conn          | explicit `Cancel` frame  |
| Per-op rate limit    | layered route | dispatch-side     | layered route       | dispatch-side            |
| Error surface        | HTTP status   | per-frame `Error` | mid-stream error    | `Error` frame            |
| Subscriptions        | no            | no                | no                  | yes                      |

Runtime implication: idempotency, ratelimit, and audit cannot remain
HTTP-only `tower::Layer`s. They move into a small `OpExecutor` service in
`cratestack-core` (or a new crate) that takes
`(op_id, idem_key, request_bytes, principal)` and runs the op. The HTTP
`Layer`s become thin adapters around that service; the WS dispatcher calls
the service directly.

## 5. Canonical request signing

`canonical_request_string` in `cratestack-core` is unchanged — only the
method / path / body components fed into it differ per binding (see below).

- HTTP unary and batch: the canonical request *is the actual rpc request*.
  Method `POST`, path `/rpc/<op_id>` (the concrete URL the client hit), no
  query, and the raw rpc frame bytes as the body. The frame body carries the
  primary key / patch / args, so signing it binds them.
- WS: the upgrade request is signed once via the existing
  `canonical_request_string` over the upgrade HTTP request. Frames inside
  the channel are not individually signed.

No new signing primitives are introduced.

**Canonical request under `transport rpc` is the concrete rpc request, not
the REST shape.** On RPC dispatch the canonical fed into *both* signature
verification (`request_context`) and the `cratestack_route` tracing field is:

- **method** = `POST`
- **path** = `/rpc/<op_id>` — the real URL, e.g. `/rpc/procedure.<name>`,
  `/rpc/model.<M>.{list,get,create,update,delete}`. NOT the bare op id and
  NOT the REST `/$procs/<name>` or `/<plural>[/<id>]` shape.
- **query** = none
- **body** = the raw rpc frame bytes the dispatch received (the unwrapped
  unary payload, or the `{id}` / `{id, patch}` frame for CRUD verbs) —
  *before* any re-decoding. This is what binds the id / patch / args to the
  signature, so e.g. `model.<M>.get` with a different `id` is a different
  signed request.

This matches the rpc client byte-for-byte — it signs `path =
format!("/rpc/{op_id}")`, method `POST`, with the same frame body
(`cratestack-client-rust/src/rpc/client.rs`). On the REST binding the
canonical remains the REST method / path / query / body, byte for byte as
before.

## 6. What is explicitly out of scope for v1

These are deliberate non-features. Revisit only when concrete user demand
appears.

- **Resumable subscriptions.** No cursors, no replay. Fire-and-forget only.
- **In-batch transactional mode.** Each batch frame is its own tx.
- **In-batch dependencies.** No `$ref` to a sibling frame's output.
- **Per-frame signing in WS sessions.** Channel auth at upgrade is the
  only model.
- **HTTP/2 server push** as a streaming transport. SSE and cbor-seq cover
  the use cases; H/2 push is being deprecated in the broader ecosystem.
- **~~Subscriptions over SSE/cbor-seq. WS only.~~ Revised — see §3.4a.**
  SSE is now a first-class subscription binding for the fire-and-forget
  case; WS remains for bidirectional/high-multiplexing needs.
- **Cross-schema dispatch.** Each schema has its own op registry; mounting
  two schemas in one binary produces two independent registries under
  different prefixes.

## 6.5. WebSocket + SSE subscription bindings — status

§3.4 specifies the wire shape for WebSocket subscriptions; §3.4a (added by
issue #183's spike decision) specifies SSE as the recommended first path.
**SSE shipped in cratestack#390.** WS remains unimplemented. Unlike
streaming — where list-return procedures had a concrete shape (paginated
reads, audit feeds, anything naturally producing a finite sequence) and
the binding fell out of the existing axum sequence encoder —
subscription use cases haven't crystallized in the CrateStack consumer
base yet; SSE was built anyway per #183's "implementation cost is low
enough" reasoning below, not because a driving case appeared.

What shipped, concretely:

- **Schema directive.** `@@subscribe` (bare, model-level; mirrors
  `@@audit`/`@@soft_delete`) parses in `cratestack-parser::validate::
  model_attributes`, requiring `@@emit(...)` on the same model and
  `transport rpc` on the schema. Emits the `OpKind::Subscription`
  `model.<X>.subscribe` op descriptor
  (`crates/cratestack-macros/src/transport/op_descriptors.rs`).
- **SSE dispatch.** `GET /rpc/subscribe/{op_id}` is wired up
  (`crates/cratestack-macros/src/include/server/rpc_module/subscribe.rs`
  generates the per-model dispatch arm;
  `crates/cratestack-axum/src/rpc/sse.rs` is the encoder). Auth is
  header-based via the existing `AuthProvider`, matching every other
  HTTP RPC binding — no upgrade-time HMAC.
- **Bus integration.** `CoolEventBus::subscribe` now returns a
  `SubscriptionHandle`, removable via the new `CoolEventBus::
  unsubscribe`; `cratestack_core::SubscriptionGuard` unsubscribes every
  tracked handle on drop, whichever way the SSE stream ends (overflow or
  client disconnect). Per-client fan-out and the bounded send buffer
  live in `crates/cratestack-axum/src/rpc/subscription_bridge.rs` —
  overflow closes the channel, which the encoder turns into a terminal
  `Error{code:"unavailable"}` SSE event. Delivery itself reuses the
  outbox-drain path `@@emit` already had (no new pipeline): a mutating
  op's transaction commit already calls `drain_event_outbox()`, which
  now also feeds any live SSE subscribers registered on that topic.

Still missing:

- **WS frame loop.** The `Request`/`Response`/`StreamItem`/`StreamEnd`/
  `Cancel`/`Error` variants in §2.3 are not wired through to the axum WS
  extractor. Deferred until a concrete bidirectional/multiplexing case
  appears, per §3.4a.
- **Row-level policy on streamed events.** `@@allow` is not replayed
  against `ModelEvent<T>` items — a subscriber only needs to
  authenticate, it does not get per-row filtering. Documented scope
  limit, not a gap in the SSE path itself; revisit if a concrete case
  needs it.
- **Client-generated subscription helpers** (Rust/TypeScript/Dart). Out
  of scope for #390 by design — server-side dispatch ships first, per
  this repo's established pattern (e.g. #171 before #209/#210 for gRPC).

This "no concrete consumer yet" gate still fully applies to the **WS**
path (§3.4) — server-to-server consumers in CrateStack's audit/event
landscape today don't need subscriptions, they poll or consume from the
audit sink, and building the bidirectional/multiplexing machinery WS
offers without a concrete case for it would be speculative. It did
**not** apply the same way to the **SSE** path (§3.4a): the
implementation cost was low enough (reusing shipped machinery, no new
frame types) that issue #183 recommended scoping and building it as a
follow-up regardless, rather than waiting on the same trigger WS is
gated on. External clients (mobile apps, browser SPAs) remain the
natural fit for either path, whenever one materializes.

## 7. Compatibility

`transport` defaults to `rest` when omitted. Schemas authored before this
directive existed parse unchanged with REST behavior. The snapshot format
version is not bumped: `Schema.transport` is `#[serde(default)]`, so old
snapshots load with `TransportStyle::Rest`.

Clients (`cratestack-client-{rust,typescript,dart,flutter}`) inspect
`Schema.transport` at codegen time and emit either a REST client or an
RPC client. There is no client that speaks both.
