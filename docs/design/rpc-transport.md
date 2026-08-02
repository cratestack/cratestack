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
| WebSocket binding + subscriptions (`@@subscribe` schema directive) | **pending** | — |
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
