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
| Streaming for `Sequence`-kind ops via `Accept: application/cbor-seq` | shipped | #24 (test coverage only — no code change needed) |
| WebSocket binding + subscriptions (`@@subscribe` schema directive) | **pending** | — |
| Batch parallelization | deferred (no observed contention) | — |

Streaming turned out to be free: list-return procedures already get
`OpKind::Sequence` from the macro, and the existing axum handler does
content-negotiated `application/cbor-seq` encoding. The RPC dispatcher
delegates unchanged, so no new code path was needed beyond the test
fixture that pins the contract.

Subscriptions are the only HTTP-surface gap left, and unlike streaming
the use cases are not yet concrete enough to motivate the schema-syntax
and runtime work — see §6.

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
- Errors mid-stream are signaled by a trailing
  `application/cratestack.error+cbor` chunk (SSE: `event: error`).
- **No subscriptions over HTTP streaming.** SSE cancellation is "close the
  connection," which races with backpressure on the server side. Subscriptions
  live on WS only.

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

`canonical_request_string` in `cratestack-core` is unchanged.

- HTTP unary and batch: signed exactly as REST today. Body bytes already
  cover everything an attacker could mutate.
- WS: the upgrade request is signed once via the existing
  `canonical_request_string` over the upgrade HTTP request. Frames inside
  the channel are not individually signed.

No new signing primitives are introduced.

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
- **Subscriptions over SSE/cbor-seq.** WS only.
- **Cross-schema dispatch.** Each schema has its own op registry; mounting
  two schemas in one binary produces two independent registries under
  different prefixes.

## 6.5. WebSocket binding + subscriptions — status

§3.4 specifies the wire shape for WebSocket and subscriptions in detail.
None of it is implemented yet. Unlike streaming — where list-return
procedures had a concrete shape (paginated reads, audit feeds, anything
naturally producing a finite sequence) and the binding fell out of the
existing axum sequence encoder — subscription use cases haven't
crystallized in the CrateStack consumer base yet. The design captured in
§3.4 stays as the target; the runtime work is gated on a real driving
case.

Concretely, what's missing:

- **Schema directive.** `@@subscribe` on models doesn't parse today;
  `OpKind::Subscription` exists in `cratestack-core` but no `.cstack`
  syntax emits it.
- **WS frame loop.** The `Request`/`Response`/`StreamItem`/`StreamEnd`/
  `Cancel`/`Error` variants in §2.3 are not wired through to the
  axum WS extractor.
- **Bus integration.** `CoolEventBus` already exists in
  `cratestack-core` and is what a subscription would ride on, but the
  per-client fan-out + bounded-buffer behavior described in §3.4 needs
  to be written.

The honest question to ask before that work starts is *what subscription
should I implement, for whom*. Server-to-server consumers in
CrateStack's audit/event landscape today don't need subscriptions — they
poll or consume from the audit sink. External clients (mobile apps,
browser SPAs) are the natural fit, but no concrete CrateStack consumer
is asking for them yet. When one does, this section becomes a v1 task.

## 7. Compatibility

`transport` defaults to `rest` when omitted. Schemas authored before this
directive existed parse unchanged with REST behavior. The snapshot format
version is not bumped: `Schema.transport` is `#[serde(default)]`, so old
snapshots load with `TransportStyle::Rest`.

Clients (`cratestack-client-{rust,typescript,dart,flutter}`) inspect
`Schema.transport` at codegen time and emit either a REST client or an
RPC client. There is no client that speaks both.
