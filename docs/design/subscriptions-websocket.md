# Subscriptions: a WebSocket binding, subscription procedures, and multi-replica delivery

Status: **proposed** (2026-10-28). Built so far: the WebTransport binding runtime (§11 S5b, crate
`cratestack-webtransport`), with the S5b interop spike's results recorded in §6a.5. Nothing else
here is built yet. It extends — it does not replace —
[rpc-transport.md](rpc-transport.md) §3.4 (the WebSocket binding, specified since 2026-05-15 and
never built) and §3.4a (the SSE binding, shipped in cratestack#390).

Scope: the `.cstack` grammar for subscriptions, `cratestack-macros` codegen for all subscription
dispatch, `cratestack-axum`'s SSE and (new) WebSocket bindings, model-event delivery in
`cratestack-sqlx`, a new L1 delivery contract in `cratestack-core`, `cratestack-policy`, and the
Rust, TypeScript and Dart clients (plus their swr / react-query / riverpod layers).

**Evidence convention.** `path:line` anchors were READ on `main` at `3b19c3e` (0.16.0 + Unreleased,
2026-10-28). Nothing in this document was RAN; every behavioral claim about today's code comes from
reading it, and each slice in §11 owes its own test evidence.

## Summary

| Question | Decision proposed |
|---|---|
| What is the "new kind" of subscription? | A **`subscription procedure`**: a schema-declared, policy-guarded, argument-taking op whose handler returns a stream of an application type. Model change feeds (`@@subscribe`) stay, and gain an input (filter, operations, projection). See §3. |
| Where do app-defined events come from? | Typed **`topic`** declarations: `db.topics().room_message(&room_id).publish(msg)` on one replica reaches `subscribe()` on every replica. See §3.4. |
| WebSocket? | Yes: `GET /rpc/ws` (and `GET /$ws` for REST schemas), one socket multiplexing many subscriptions, using the §2.3 frame envelope plus two new variants (`Init`, `Ready`). v1 carries **subscriptions only**; unary ops over WS are a later, separately gated slice. See §6. |
| SSE? | Stays first-class. Gains input, a `ready` event, and keepalives, so both bindings expose the same contract. See §7. |
| Multi-replica? | A new L1 contract, **`SubscriptionBackplane`**, with in-process, Postgres `LISTEN/NOTIFY` and Redis pub/sub implementations. Model events are `pg_notify`'d **inside the writing transaction**, so every replica sees exactly the committed events. See §4. |
| Access control? | Opening a subscription evaluates policy; every model event is checked against the model's **read** policy for *that* subscriber before it is sent. Policies that cannot be decided in memory are refetched through the policy-scoped `get`. See §5. |
| Delivery guarantee | Still fire-and-forget, no replay — but with one new invariant that makes it usable: **a live subscription has missed nothing since its `Ready`; any loss ends it with `unavailable`.** See §2.1. |
| REST vs RPC | Both, in the same slices (CLAUDE.md, "Transport parity"). The `transport rpc` requirement on `@@subscribe` is lifted. See §7.2. |
| Durable / offline delivery | Out of scope here. That is `cratestack-sync`'s job (ADR 0020); §12 explains where the line sits. |

## 1. What exists today

| Piece | Where | What it does |
|---|---|---|
| `@@subscribe` | `cratestack-parser/src/validate/model_attributes.rs:81` | Bare model attribute. Requires `@@emit(...)` on the model and `transport rpc` on the schema. |
| Op | `cratestack-core/src/op_list.rs:28` | `ModelVerb::Subscribe` → `model.<X>.subscribe`, `OpKind::Subscription`. "Only under `transport rpc`." |
| Dispatch | `cratestack-macros/src/transport/subscribe_dispatch.rs` | `GET /rpc/subscribe/{op_id}`: authenticate, register one bus handler per emitted operation, stream. Takes no input. |
| Bus | `cratestack-core/src/events/bus.rs:27` | `CratestackEventBus`: in-process, topic → handlers. `emit` awaits handlers one after another (`:82-96`). |
| Backpressure | `cratestack-axum/src/rpc/subscription_bridge.rs:27` | Bounded channel of 64 per subscription; overflow closes it. |
| Encoder | `cratestack-axum/src/rpc/sse.rs` | `event: message` per item, a final `event: error` `unavailable` on overflow. JSON only. No keepalive. |
| Delivery | `cratestack-sqlx/src/descriptor.rs:124` | `drain_event_outbox`, called after each mutating op commits (`query/write/create.rs:166` and seven siblings). |
| Outbox write | `cratestack-sqlx/src/descriptor/event_outbox.rs:95` | `enqueue_event_outbox`: one `INSERT` in the writer's transaction. The single choke point every emitted event passes through. |
| Clients | — | None. No generated client can consume a subscription (rpc-transport.md §6.5, "Still missing"). |

What that shape cannot do:

1. **Only model CRUD events.** A chat message, a job's progress, a presence change, a computed
   aggregate — anything that is not "row X was created/updated/deleted" — has no way in.
2. **Single replica only.** The bus is in-process, and `drain_event_outbox` runs on the replica that
   committed the write. A subscriber connected to replica B never sees a write made on replica A.
   Separately, the drain's `SELECT … WHERE delivered_at IS NULL` (`descriptor.rs:133-138`) takes no
   row lock, so two replicas draining at once can both emit the same row to in-process handlers.
3. **No input.** A client cannot say "only `Order`s for my store" or "only `deleted`". Every
   subscriber receives every event of the model.
4. **One connection per subscription.** Fine under HTTP/2 (rpc-transport.md §3.4a), but a browser on
   HTTP/1.1 is capped at six connections per origin, and a mobile client holding five feeds holds
   five sockets.
5. **RPC only.** A REST schema cannot declare `@@subscribe` — a standing exception to the parity
   rule CLAUDE.md now states.
6. **No client side at all**, and no keepalive, so an idle SSE stream behind a proxy with a 60 s idle
   timeout is silently cut.

The row-level-policy limit in rpc-transport.md §3.4a is real too. §5 replaces that "documented scope
limit" with an enforced model: subscriptions see exactly what `list`/`get` would show the same
caller.

## 2. Goals and non-goals

Goals:

- **G1. A WebSocket binding** (rpc-transport.md §3.4) that multiplexes many subscriptions over one
  authenticated socket.
- **G2. A new kind of subscription**, application-defined, with arguments and policy, for anything
  that is not a model row change.
- **G3. Multi-replica correctness**: a subscriber sees an event wherever it was committed.
- **G4. Access control equal to reads**: no subscription shows a caller a row `get` would refuse.
- **G5. Parity**: REST and RPC, SSE and WS, Rust/TS/Dart — one contract, several bindings.
- **G6. Usable clients**, including the "subscribe, then load the snapshot" pattern done without a race.

Non-goals (each revisited in §12):

- Resumable subscriptions, cursors, replay. Still out (rpc-transport.md §6).
- Unary and `@stream` ops over WebSocket in v1.
- The embedded role (`include_embedded_schema!`): no network, no policy, different problem.
- MCP: exempt by ADR 0002 D5.
- Wire compatibility with `graphql-ws` or tRPC's protocol.

### 2.1 The delivery contract

Fire-and-forget stays. What changes is that loss becomes **observable**:

> From the moment a subscription is `Ready`, it either delivers every event it is entitled to, in
> commit order, or it ends with `Error{code: "unavailable"}`. It never silently skips.

Every way an event can be lost ends the subscription: a full per-subscription buffer (as today), a
backplane listener that reconnected and may have missed notifications (§4.5), a replica shutting
down. With that guarantee, a client gets consistency from fire-and-forget delivery like this:

1. Subscribe, and wait for `Ready`.
2. Load the snapshot (`list`/`get`).
3. Apply events. Discard any whose `occurred_at` is older than the snapshot's row version (or simply
   re-apply them, since events carry full rows and are idempotent upserts by key).
4. On `unavailable`, go to 1.

Steps 1–4 ship as a client helper (§8), so applications do not each rebuild them.

## 3. Schema surface

### 3.1 Model feeds: `@@subscribe`, now with input

The spelling does not change. What changes is that the op takes an input:

```text
SubscribeInput {
  where?:      <the model's list filter, scalar subset — §5.3>,
  operations?: ["created" | "updated" | "deleted"],   // default: all the model emits
  fields?:     [<field>],                              // projection, same rules as list
}
```

On RPC it travels in the request frame. On REST it travels in the query string, using the same
syntax `list` already parses. RPC synthesizes that query string and re-enters the REST parser
(`cratestack-axum/src/rpc/synthesize.rs`), the mechanism CLAUDE.md names as what makes parity
cheap. Item type: `ModelEvent<X>`, unchanged
(`cratestack-core/src/events.rs:56`). An empty input is today's behavior, so existing callers keep
working.

### 3.2 Subscription procedures (new)

```text
type RoomArgs {
  roomId String
}

type ChatMessage {
  id       Uuid
  roomId   String
  authorId Int
  body     String
  sentAt   DateTime
}

subscription procedure roomMessages(args: RoomArgs): ChatMessage
  @allow(auth() != null)
```

The policy guards the op. Membership in the room is data, so the handler checks it during setup
and refuses with `permission_denied` before `Ready` (§3.3).

Rules, enforced by `cratestack-parser`:

- `subscription` is a procedure modifier, a sibling of `mutation` (`mutation procedure foo(...)`
  already parses). The two are mutually exclusive.
- The return type is a single `T` — the item type. `T[]` is rejected, because it reads as "a stream
  of arrays" or "a finite list", and neither is what is meant.
- `@stream`, `@isolation`, `@idempotent` and `@no_idempotency` are rejected. A subscription is
  already a stream, holds no transaction open for its lifetime, and has no replayable result.
- `@allow` / `@deny` use the procedure dialect and are evaluated once, at open, against `args` and
  `auth()` — the existing `authorize_procedure` path (`cratestack-policy/src/eval.rs:11`).
- Allowed in `db = None` schemas (`cratestack-api`), where a procedures-only service with live
  updates is a natural case.

Op id: `procedure.roomMessages`, `OpKind::Subscription` — the same "id names the callable, kind is
metadata" rule the `@stream` row of rpc-transport.md §2.1 follows.

### 3.3 The handler

```rust
fn room_messages(
    &self,
    db: &super::Cratestack,
    ctx: &::cratestack::CratestackContext,
    args: room_messages::Args,
    _authorized: room_messages::Authorized,
) -> impl Future<Output = Result<
    ::cratestack::SubscriptionStream<room_messages::Item>,
    ::cratestack::CratestackError,
>> + Send;

// in cratestack-core:
pub type SubscriptionStream<T> =
    Pin<Box<dyn Stream<Item = Result<T, CratestackError>> + Send + 'static>>;
```

It is shaped differently from the `@stream` method
(`cratestack-macros/src/procedure/registry_method.rs:48-57`, which returns the stream directly) for
two reasons:

- **Two phases.** Setup can fail ("room not found", "not a member") and must fail as an ordinary
  error *before* `Ready`. A bare stream could only fail as its first item, which a client cannot
  tell apart from a mid-stream failure.
- **`'static`.** A subscription outlives the request borrow, so the stream must own what it needs.

The `Authorized` token keeps cratestack#512's guarantee: a handler cannot be called without the
policy having run.

A stream that ends on its own sends `StreamEnd`. A stream that yields `Err` sends that error and
ends. Client disconnect or `Cancel` drops the stream, which is how a handler cleans up (RAII, the
same as `SubscriptionGuard` today).

### 3.4 Topics (new): typed, cross-replica publish

Most subscription procedures do the same thing: forward messages that some other code path
publishes. A topic declares the key and payload type once:

```text
topic RoomMessage(roomId: String): ChatMessage
```

and generates:

```rust
// publish from anywhere: a mutation procedure, a webhook handler, a job
db.topics().room_message(&room_id).publish(&msg).await?;

// publish so delivery happens only if this transaction commits (§4.3)
db.topics().room_message(&room_id).publish_in_tx(&mut tx, &msg).await?;

// subscribe — what most handlers return
db.topics().room_message(&args.room_id).subscribe()   // -> SubscriptionStream<ChatMessage>
```

The handler for §3.2 is then one line. A topic is server-internal: it is not an op, has no route and
no policy of its own. The `subscription procedure` that exposes it is the policy boundary.
Everything on the wire goes through a declared op.

**Declarative shorthand (later slice, §11 S7).** `subscription procedure roomMessages(args:
RoomArgs): ChatMessage @topic(RoomMessage, args.roomId)` generates the handler. It is listed so the
grammar leaves room for it, not because v1 needs it.

## 4. Delivery: the backplane

### 4.1 The contract

A new L1 contract beside `IdempotencyStore` and `RateLimitStore` (`cratestack-core/src/store/`,
following ADR 0016's split between the trait and its swappable implementations):

```rust
pub trait SubscriptionBackplane: Send + Sync + 'static {
    /// Fire-and-forget publish to every replica (including this one).
    fn publish(&self, channel: &str, payload: Bytes) -> BoxFuture<'_, Result<(), CratestackError>>;

    /// Receive everything published on `channel` from now on. `Lagged`/`Resync`
    /// are how the backplane reports that events may have been lost (§4.5).
    fn subscribe(&self, channel: &str) -> BackplaneReceiver;
}

pub enum BackplaneMessage { Payload(Bytes), Resync }
```

| Implementation | Crate (layer) | Use |
|---|---|---|
| `InProcessBackplane` | `cratestack-core` (L0/L1) | Default for `cratestack-api` and tests; single replica. |
| `PgNotifyBackplane` | `cratestack-sqlx` (L2) | Default for `cratestack-pg`. One `PgListener` connection per process, the pattern `cratestack-sync` already ships (`cratestack-sync/src/server/poke.rs`). |
| `RedisBackplane` | `cratestack-redis` (L2) | Deployments that already run Redis for idempotency or rate limits, or that publish at high volume. |

### 4.2 One listener per replica, fan-out locally

Each replica holds **one** backplane subscription per channel that has at least one local subscriber,
and fans out in memory to the per-subscription bounded channels that already exist
(`SubscriptionPush`, `subscription_bridge.rs:34`). A `LISTEN` per client would use one Postgres
connection per open subscription. The local fan-out is reference-counted: the last local unsubscribe
releases the channel.

### 4.3 Model events: `pg_notify` inside the writing transaction

With `PgNotifyBackplane`, `enqueue_event_outbox` (`event_outbox.rs:95`) issues
`SELECT pg_notify('cratestack_events', $payload)` on the **same executor** right after its `INSERT`,
for models that declare `@@subscribe`. Postgres delivers a notification only when its transaction
commits, to every listening session, in commit order. So:

- no rolled-back write is ever announced (no dual-write problem);
- every replica sees every committed event, including the one that wrote it;
- no drain, no lock, no `delivered_at` bookkeeping on this path.

The payload limit is 8000 bytes. Above roughly 7 KiB the notification carries
`{event_id, model, operation}` only, and the receiving replica reads the row from
`cratestack_event_outbox` by `event_id`. The row is guaranteed to be there, because it was inserted
in the same transaction.

`publish_in_tx` on a topic (§3.4) is the same mechanism with an application payload.

**Two paths, two guarantees, on purpose.** In-process `on_<model>_<op>` handlers keep riding
`drain_event_outbox`: they are at-least-once, retried, and (this design proposes, §11 S2) drained
with `FOR UPDATE SKIP LOCKED` so two replicas never both deliver one row. Subscribers ride the
notify path: at-most-once, observable loss (§2.1). Putting subscribers on the drain path would give
them duplicates and cross-replica races. Putting handlers on the notify path would drop their
retries.

With `RedisBackplane`, model events are published after commit, from the same place
`drain_event_outbox` is called today. A crash between commit and publish loses the event. Under
§2.1 that is acceptable, but unlike the Postgres path it is *not* observable. The Redis
implementation's docs must say so plainly.

### 4.4 Ordering

Per replica, events reach a subscription in the order the backplane delivered them. That is commit
order for `PgNotifyBackplane`, and publish order per channel for Redis. Topics have no ordering
across keys.

### 4.5 Listener gaps

When a backplane listener reconnects, it cannot know what it missed. It emits `Resync`, and every
local subscription on that channel ends with `Error{code: "unavailable", message: "subscription
interrupted"}`. This is the same signal as a lagged buffer, and clients already handle it by going
back to step 1 of §2.1. `cratestack-sync`'s poke hub makes the same move on reconnect: it pokes
every stream (`poke.rs:20`, `ALL_SCOPES`, sent at `:67`).

## 5. Access control

### 5.1 At open

- **Subscription procedures:** the procedure's `@allow`/`@deny`, evaluated against `args` and the
  authenticated context. Refusal → `permission_denied`, before `Ready`.
- **Model feeds:** the model's `read` policy, evaluated with every row-dependent predicate as
  `Unknown`, using the Kleene logic `cratestack-policy` already has (`truth.rs:20`). If the result
  is `False` — for example `auth() != null` for an anonymous caller — refuse with
  `unauthenticated`/`permission_denied`. If it is `True` or `Unknown`, open the subscription and
  decide per event (§5.2).

The authenticated context is captured once and used for every event of that subscription. §6.3
covers credential expiry.

### 5.2 Per event, for model feeds

`@@allow("read", …)` is compiled into SQL today. No in-memory evaluator exists
(`cratestack-policy/src/lib.rs` exports only procedure-dialect evaluators). The macro classifies
each model's read policy at compile time:

| Class | Read policy uses | Per-event check | Cost |
|---|---|---|---|
| **A: row-local** | the row's own scalar columns, `auth()`, literals | New in-memory `ReadPolicy` evaluator in `cratestack-policy`, run against the event's row | One evaluation per (event, subscriber). No I/O. |
| **B: relational** | relation quantifiers, anything not decidable from the row alone | **Refetch**: run the policy-scoped `get` by primary key; deliver what it returns, drop the event if it returns nothing | One query per (event, *auth-equivalence group*) |

Class A's evaluator must agree with the SQL compiler. The slice that adds it also adds a
differential test: generated rows × generated contexts, evaluated both in memory and in Postgres,
and the two must agree on every case. Any disagreement fails the build.

Class B groups subscribers by the values of the `auth()` fields the policy references, not by
subscriber. 500 subscribers in 3 tenants cost 3 queries per event, not 500. Refetch also delivers
the freshest row, which is the strategy `cratestack-sync` uses for the same reason ("pulling stays
the only path rows take", sync-engine.md §poke).

**Deletes.** The delete event carries the deleted row (`query/write/delete.rs:102-107`), so class A
evaluates it as is. Class B cannot refetch a deleted row. It delivers a key-only tombstone, and only
to subscriptions that previously delivered that key (a bounded per-subscription seen-set; overflow
ends the subscription with `unavailable`, per §2.1). Decision O2 asks whether that is worth its
complexity versus refusing deletes for class B.

**Projection.** `fields` and any field-level read restriction `list`/`get` apply are applied to the
delivered row in exactly the same way. The rule is that a subscription never shows a field a `get`
by the same caller would not.

### 5.3 Filters

`where` accepts the scalar subset of the list filter grammar (equality, `in`, comparisons, null
checks, `and`/`or`/`not` over the model's own scalar fields), evaluated in memory with the class-A
evaluator. A relation filter is refused at open with `invalid_argument`.

A filter is checked against the event's row: the post-image for create and update, the pre-image for
delete. One consequence: a row that is *updated out of* the filter produces no event for that
subscriber, so the subscriber's copy goes stale. The pre-image already exists for audit on the
update path loads a pre-image today, but only when `@@audit` is on (`query/write/update.rs:91`).
Loading it for every `@@subscribe` model as well costs one extra read per update, and makes
"left the filter" events (`ModelEvent { operation: Updated, matches: false }`) buildable.
Decision O3.

## 6. The WebSocket binding

### 6.1 Endpoint

- `GET /rpc/ws` for `transport rpc` schemas, `GET /$ws` for REST schemas. Both speak the same frame
  protocol, keyed by op id. Op ids are already transport-independent
  (`cratestack-core/src/op_list.rs:1-13`: "the one list of ops a schema exposes").
- Generated only when the schema has at least one subscription op and the `ws` Cargo feature is on.
  §9 covers gating.

### 6.2 Subprotocol and framing

As rpc-transport.md §3.4 specifies: offers `cratestack.rpc.v1+cbor` and `cratestack.rpc.v1+json`,
and the server picks one or closes with 1002. One frame per WS message: binary frames carry CBOR,
text frames carry JSON. Unlike SSE, WS carries CBOR natively, which matters for `Decimal`, `BigInt`
and byte fields.

### 6.3 Authentication

rpc-transport.md §3.4 says "the upgrade request is HMAC-signed". That works for native clients and
servers, and stays. It does not work for browsers, whose `WebSocket` constructor cannot set headers.
(The same constraint made `cratestack-sync` read its poke stream with `fetch`,
`cratestack-sync/src/server/http/stream.rs:5-6`.) So there are two ways in:

1. **Upgrade headers** (Rust, Dart, server-to-server): `AuthProvider::authenticate` over the upgrade
   request's canonical form (`GET /rpc/ws`, no body), including HMAC envelopes.
2. **`Init` frame** (browsers): the first frame within 10 s must be
   `Init { init: { headers: { "authorization": "…" } } }`. The server runs the same `AuthProvider`
   over a synthesized request context with those headers, and answers `Ready { id: 0 }` or refuses
   (below). No other frame is accepted before that.

Tokens in the URL query (they end up in access logs) and in `Sec-WebSocket-Protocol` (a misuse of the
header that some proxies log too) are both rejected as designs.

**Origin.** If the `AuthProvider` can accept ambient credentials (cookies), cross-site WebSocket
hijacking is possible. The upgrade checks `Origin` against an allowlist (`WsConfig::allowed_origins`).
The default is same-origin only, and the check is never skipped silently.

**Refusal (ADR 0021 §1, cratestack-docs `internals/auth-seam-adr.md`).** A refusal is never just a
close code. The server first sends `{id: 0, err: RpcErrorBody}`, built by
`RpcErrorBody::from_cratestack`, so an `AuthRefusal`'s `details` and extra members (the `error`
reason a device-signature client switches on) arrive as they do on REST and RPC. Then it closes:

| Refusal | Close |
|---|---|
| 401 (`Unauthorized`, `AuthRefusal::unauthenticated`) | 4401 |
| 403 (`Forbidden`, `AuthRefusal::forbidden`), or a re-authentication naming another principal | 4403 (ADR 0021 Q4) |
| a malformed `Init` header | 4400 |
| the provider could not decide (5xx: a key server down) | 4500, **not** a verdict on the credentials: retry, do not sign out |

`WWW-Authenticate` challenges are not carried: a frame has no headers, and ADR 0021 Q5 keeps them out
of the body for now. A client reads the frame when present rather than the close code alone, because
4401 also follows an `Init` that never arrived. *As built* for WebTransport
(`cratestack-webtransport/src/admission.rs`): the server finishes the control stream after the frame
and waits, bounded, for the client to acknowledge it, so the session close cannot overtake it.
Verified in Chromium.

**Expiry and refresh.** The client may send `Init` again on a live socket to refresh credentials.
A refused refresh is refused as above. The new context must name the same principal, or it is a 403
refusal (4403). If the context carries an expiry, the server closes with 4401 at that instant unless a
refresh arrived first.

### 6.4 Frames

The §2.3 envelope plus two variants, `Init` and `Ready`. The envelope is discriminated by which key
is present, so new variants are additive. The key names are the ones that already ship, not §2.3's
older sketch (`in`/`out`): a request is the batch binding's `RpcRequest` (`id`, `op`, `input`), and
items and errors are the SSE binding's `next`/`err` (`cratestack-axum/src/rpc/sse.rs`). *As built*
in `cratestack-webtransport/src/frame.rs`:

```text
Init      { id: 0, init: { headers: { <name>: <value> } } }  client → server
Request   { id: u64, op: string, input: <value> }             client → server
Ready     { id: u64, ready: {} }                              server → client
StreamItem{ id: u64, next: <value> }                          server → client
StreamEnd { id: u64, end: {} }                                server → client
Error     { id: u64, err: RpcErrorBody }                      server → client
```

A `next` that is present but `null` is an item whose value is `null`. Presence decides the kind,
never the value.

`Ready{id: 0}` acknowledges `Init`. `Ready{id: n}` says subscription `n` is live: the §2.1 guarantee
starts at this frame.

One subscription's life:

```text
C→S  Request    { id: 7, op: "procedure.roomMessages", input: { roomId: "r1" } }
S→C  Ready      { id: 7, ready: {} }                    | Error { id: 7, err } (policy, setup)
S→C  StreamItem { id: 7, next: <ChatMessage> } …
C→S  Cancel     { id: 7, cancel: {} }                   (optional)
S→C  StreamEnd  { id: 7, end: {} }                      | Error { id: 7, err: unavailable… }
```

An `id` already in use gets `Error{invalid_argument}` and leaves the existing subscription alone. A
`Request` naming a non-`Subscription` op gets `Error{invalid_argument, "op kind not supported over
WebSocket"}` in v1 (§6.7).

### 6.5 Keepalive

WS ping every 15 s. No pong within 30 s closes the socket. SSE (§7) sends a comment line at the same
interval. The interval matches `cratestack-sync`'s `KeepAlive` (`http/stream.rs:41`).

### 6.6 Limits

| Limit | Default | Exceeded → |
|---|---|---|
| Subscriptions per socket | 64 | `Error{resource_exhausted}` on that `Request`; socket stays |
| Sockets per principal | 8 | Close 4429 at `Init`/upgrade |
| Inbound frame size | `DEFAULT_BODY_LIMIT_BYTES` (2 MiB, request-response-size-bounds.md) | Close 1009 |
| Per-subscription buffer | 64 events (as today) | That subscription ends `unavailable`; socket stays |
| Outbound socket queue | 1024 frames | Close 1013 (try again later); every subscription is lost, and the client knows |

All are `WsConfig` fields, not `.cstack` syntax, consistent with extensions.md's rule that tuning
numbers never enter the schema.

### 6.7 Unary ops over WS: deliberately later

§2.3's `Request`/`Response` pair already describes unary calls over a socket, and the §4 table
already assigns per-frame idempotency and dispatch-side rate limiting to it. Building that means
routing every WS request through `cratestack-exec`'s `OpExecutor`, which today does admission only
(ADR 0015) and is not yet the full op runner. Subscribing does not need it. v1 routes the
subscription `Request` through `OpExecutor::admit_rate_limit` (dispatch-side, per the §4 table) and
nothing else. Unary-over-WS stays gated on the trigger rpc-transport.md §6.5 already names: a
concrete multiplexing need.

## 6a. The WebTransport binding

WebTransport is a first-class binding, not a future option. This is a deployment requirement: the
production app this design serves is built on it. Its server and its clients ship in the same
release train as WS. What follows maps the subscription contract onto it and is explicit about what
it costs to run, because those costs are real.

### 6a.1 What it gives a subscription that WS cannot

- **One stream per subscription, no head-of-line blocking.** A WS socket is one TCP byte stream: a
  lost packet or a large item on one subscription stalls every other subscription on the socket.
  Over QUIC each subscription is its own stream. Loss and flow control are per stream.
- **Sessions survive network changes.** QUIC connection migration keeps the session alive when a
  phone moves from Wi-Fi to cellular. Over WS that ends every subscription, and §2.1 then makes the
  client resubscribe and reload each snapshot.
- **The transport does the multiplexing.** The `id` field and the `Cancel` frame exist on WS only
  because subscriptions share one socket. Here, the stream is the identity and resetting it is the
  cancel.
- **Datagrams**, for the one kind of data where dropping is correct (§6a.7).

### 6a.2 Session and streams

```text
client                                              server
  │  HTTP/3 extended CONNECT  :protocol=webtransport  │   /rpc/wt  (REST schemas: /$wt)
  │ ────────────────────────────────────────────────▶ │   Origin + (native) auth headers checked
  │ ◀──────────────────────────────────────── 200 ─── │
  │                                                   │
  │  1st bidi stream (control): Init ─▶ ◀─ Ready{0}   │   auth, later re-auth; nothing else
  │  each later bidi stream = one subscription:       │
  │     Request ─▶  ◀─ Ready{n}  ◀─ StreamItem …      │
  │     ◀─ StreamEnd | Error, then FIN                │
  │     RESET_STREAM / STOP_SENDING  ─▶  = Cancel     │
```

- **Session = the WS socket.** It holds the authenticated context, the principal, and the
  per-session limits.
- **Control stream.** The first client-opened bidirectional stream carries `Init` → `Ready{id: 0}`
  and later re-auth `Init`s (§6.3's rules, unchanged). No subscription stream is accepted before
  `Ready{id: 0}`.
- **One bidirectional stream per subscription.** The client writes exactly one `Request` frame and
  finishes its send side. The server writes `Ready`, then `StreamItem`s, then `StreamEnd` or `Error`,
  then FIN. Either side resetting the stream ends the subscription. A client reset is `Cancel`. A
  server reset (code `0x01`) means the session is going away, and the client treats it as
  `unavailable`.
- **`id` stays in the frames.** That keeps the §2.3 codec byte-identical across bindings, so all
  three share one encoder and decoder. On WebTransport the stream routes, and an `id` that does not
  match the stream's `Request` is a protocol error (stream reset, code `0x02`).
- **Found by the browser test, not the native one (as built).** Two shutdown details are invisible
  to a native client and break Chromium:
  - The server reads the client's half of a subscription stream to its FIN before releasing it.
    Releasing it earlier sends `STOP_SENDING`, and a browser whose FIN for the `Request` has not
    landed yet rejects its `writer.close()`. That happens reliably for a subscription that ends at
    once.
  - A session close waits until the close capsule is out (`web-transport-quinn` writes it from a
    task it spawns). Returning first gives the browser "Connection lost" instead of the close code.

  Refused CONNECTs (403/404/406) likewise hold the connection briefly, so the status arrives before
  the connection closes.

### 6a.3 Framing and codec

A QUIC stream is a byte stream with no message boundaries, unlike a WS message. So frames are
self-delimiting:

- **CBOR (default):** frames back to back as a CBOR sequence (RFC 8742), decoded by the boundary
  scanner the streaming path already uses (`CborSeqChunkDecoder`, rpc-transport.md §3.3).
- **JSON:** one JSON document per line (NDJSON). JSON encoders escape newlines inside strings, so a
  raw `\n` only ever ends a frame.

The codec is negotiated with WebTransport application-protocol negotiation (`cratestack.rpc.v1+cbor`
/ `+json`, the WS subprotocol names) where the browser and the server crate support it. Otherwise
the client names it in the session URL (`/rpc/wt?codec=cbor`): not a secret, so the URL is an
acceptable place for it. Mid-stream errors use the `Error` frame, not tag 48900, because the frame
envelope already carries the error variant.

**One rule follows for the WS binding too:** no layer above the transport may assume one frame per
transport message. The client frame decoders are written against a byte stream from the start, so
WS and WebTransport share them.

### 6a.4 Authentication, Origin, limits

- **Native clients** (Rust, Flutter native through the Rust client, servers) send auth headers on
  the CONNECT request, which `AuthProvider::authenticate` sees as it sees a WS upgrade. The
  canonical request is `CONNECT /rpc/wt` with no body.
- **Browsers** cannot set headers on `new WebTransport(url)` either, so they use the `Init` frame on
  the control stream, exactly as §6.3 defines it.
- **Origin.** Browsers send `Origin` on the CONNECT. The same `allowed_origins` allowlist applies,
  with the same same-origin default.
- **0-RTT is refused** on this listener. Early data can be replayed, and the first bytes a client
  sends here are credentials (`Init`) or a policy-checked `Request`.
- **Limits** reuse §6.6's table, enforced by our code. The QUIC bidirectional-stream limit is a
  hard ceiling set **above** the subscription limit (twice it, plus the control and CONNECT
  streams). *As built:* setting it equal to the limit made the client's next stream open wait
  silently for credit, so the explicit `resource_exhausted` §6.6 promises could never be sent.
  Frames are capped while still incomplete, so a client cannot make the server buffer an unbounded
  frame. Keepalive is QUIC's own: PING
  every 15 s, idle timeout 30 s.
- **Backpressure is not delegated to flow control.** A client that stops reading fills its stream's
  window. That must never block the backplane fan-out (§4.2) for everyone else, so the bounded
  per-subscription buffer and its `unavailable` exit (§2.1) stay exactly as on WS.

### 6a.5 Server architecture

axum and hyper do not serve HTTP/3. So WebTransport is a **second listener in the same process**:
a UDP socket, normally on the same port number as HTTPS (`:443/udp` beside `:443/tcp`), driven by
QUIC.

- **New crate `cratestack-webtransport`, L4** (beside `cratestack-axum` and `cratestack-mcp`; added
  to `docs/adr/layers.toml` in the same PR). It depends on `cratestack-core` and `cratestack-exec`,
  never on `cratestack-axum`. It owns the QUIC endpoint, session handling, the control stream, and
  the stream ⇄ frame adapter around the generated `subscription_dispatch` (§6.1).
- **QUIC/WebTransport implementation behind our own seam.** The protocol is still an IETF draft
  (draft-ietf-webtrans-http3-16, July 2026), and the Rust crates track drafts at different speeds.
  `cratestack-webtransport` defines a small internal `WtAcceptor`/`WtSession` seam and implements it
  on one crate. A draft change then touches one module (*as built*: `src/transport.rs` is the only
  file that names the crate).
- **Decision O8, measured (2026-10-28): `web-transport-quinn` 0.13.5**, with its `ring` backend to
  match the workspace's `rustls`. The spike ran one protocol against two servers from headless
  Chromium 153 (Playwright, `serverCertificateHashes`, a page on `http://localhost`):

  | Check | `web-transport-quinn` 0.13.5 | `wtransport` 0.7.2 |
  |---|---|---|
  | Session open, `Origin` visible on CONNECT | yes | yes |
  | Subprotocol negotiation (`cratestack.rpc.v1+cbor`) | yes | **no**: the offer arrives as a raw `wt-available-protocols` header and is never answered |
  | 100 items, then a clean FIN | yes | yes |
  | Client cancel seen by the server | yes, code 0 | yes, but as the raw HTTP/3-mapped code `91141958510811` |
  | Server `reset(7)` seen by the browser | `streamErrorCode: 7` | **`streamErrorCode: 0`** |
  | 64 concurrent streams on one session | yes | yes |
  | Datagram echo | yes (browser max 1024 B) | yes |
  | Session close `(4401, reason)` seen by the browser | code and reason | **"Connection lost"**, no code |

  `wtransport` fails the three things this design relies on: negotiation (§6a.3), stream reset codes
  (§6a.2) and close codes (§6.3, §6.6). `h3-webtransport` was not measured. Its last release is
  0.1.2 (May 2025). Firefox and Safari were not measured either: only Chromium was available.
  Chromium still sends `sec-webtransport-http3-draft02: 1`, so browser and server meet on a
  draft-era wire format. That is a reason to keep the seam, and to run the browser test (§11 S5b)
  on every PR.
- **Same handle, two listeners.** The generated server exposes
  `cratestack::webtransport::serve(endpoint_config, router_state)` next to the axum `Router`. Both
  share the `AuthProvider`, backplane, `OpExecutor` and schema handle, so one replica runs one
  backplane listener (§4.2) whichever bindings its clients use.
  *As built:* `WebTransportServer::bind(WebTransportConfig, auth_provider, dispatcher)` and then
  `serve()` / `serve_with_shutdown(signal)`. The generated `serve` helper comes with S5a.
- **TLS.** The QUIC listener needs its own rustls `ServerConfig` (ALPN `h3`), built from the same
  certificate as the TCP listener. Certificate reload applies to both.
- **Local development.** Browsers accept a self-signed certificate for WebTransport only through
  `serverCertificateHashes`, which accepts only short-lived certificates (at most 14 days of
  validity; S5b's spike confirms the exact key-type rules per browser).
  `cratestack run` and the examples generate one at startup and print its hash, and the TS client
  takes it as `webtransport: { certificateHashes }`.
  *As built:* `DevCertificate::generate(hosts)` behind the crate's `dev-cert` feature. It returns
  the chain, the key and the SHA-256 hash. `examples/realtime-chat` makes one at startup and prints
  the hash (S6). `cratestack run` is not wired, because it serves the studio, not the application,
  and the studio has no subscriptions. An application server makes its own, as the example does.

### 6a.6 Running it in production

These are deployment requirements, written into the docs and the example's README, not just here:

- **UDP must reach the replicas.** L7 HTTP proxies generally do not forward WebTransport (nginx has
  no plan to). Put an **L4 UDP load balancer** in front of the replicas and terminate TLS in the
  app.
- **Migration needs a QUIC-aware balancer.** A balancer that routes by the address 4-tuple sends a
  migrated connection to a different replica, where the session does not exist. The client then
  sees the session end, gets `unavailable` everywhere, and resubscribes: correct, but it loses the
  migration benefit. To keep sessions across network changes, the balancer must route on QUIC
  connection IDs, with the server encoding a replica identifier in the connection IDs it issues
  (QUIC-LB style). The seam in §6a.5 exposes connection-ID generation for this.
- **Client IP.** There are no forwarding headers on this path, and PROXY protocol over UDP is not
  widely supported. The binding uses the QUIC peer address and needs a balancer that preserves the
  source address. `trusted-proxy-client-ip.md`'s header logic does not apply here, and rate-limit
  keys should prefer the principal.
- **Fallback is mandatory, not optional.** Some networks block UDP 443, and Safari before 26.4 has
  no WebTransport. Every generated client tries WebTransport, then WS, then SSE (§8), and the
  §2.1 contract is identical on all three, so application code cannot tell which one it got.

### 6a.7 Datagram subscriptions (opt-in)

For data where the newest value replaces the previous one and a lost update is harmless (cursor
positions, presence, live telemetry), a subscription procedure may opt out of the §2.1 guarantee:

```text
subscription procedure cursors(args: RoomArgs): CursorPosition
  @allow(auth() != null)
  @datagram
```

- Over WebTransport, each item is one QUIC datagram, prefixed with the subscription's stream id
  (varint). `Ready`, `StreamEnd` and `Error` still travel on the subscription's stream, so the
  subscription lifecycle stays reliable even though its items are not.
- An item larger than the session's datagram size limit is a handler error. It is never split, and
  it is never sent silently on the stream instead.
- Over WS and SSE the same op delivers reliably. Reliable delivery is stronger than the op asked
  for, so the op works on every binding and parity holds.
- `@datagram` is the only way to give up §2.1, and the generated client type says so
  (`LossySubscription<T>`), so nobody gets lossy delivery by accident.

## 7. The SSE binding, brought to the same contract

### 7.1 Changes

- **Input:** `POST /rpc/subscribe/{op_id}` with the input as the body (`fetch`-based SSE). `GET` keeps
  working for an empty input, so existing clients are unaffected.
- **`event: ready`** first, carrying `{"id": 1}`. This is the §2.1 start point.
- **Keepalive:** a `: keepalive` comment every 15 s.
- **Item and error events:** unchanged (`sse.rs:151-164`).
- **Codec:** still JSON. The rationale in `sse.rs`'s module doc stands. Clients that need CBOR
  fidelity use WS.

### 7.2 REST routes

| Op | REST (SSE) | RPC (SSE) | WS and WebTransport (both) |
|---|---|---|---|
| Model feed | `GET /<plural>/$subscribe?<list filter syntax>` | `GET`/`POST /rpc/subscribe/model.<X>.subscribe` | `Request{op: "model.<X>.subscribe"}` |
| Subscription procedure | `POST /$procs/<name>` + `Accept: text/event-stream` | `POST /rpc/subscribe/procedure.<name>` | `Request{op: "procedure.<name>"}` |

Session endpoints: WS at `GET /rpc/ws` / `GET /$ws`, WebTransport at `CONNECT /rpc/wt` / `CONNECT
/$wt`.

`<plural>` is `model_route_segment` (`cratestack-core/src/route_naming.rs:112`). A subscription
procedure has no unary form, so its REST route answers 406 to any other `Accept`. This is the
content-negotiation precedent `@stream` set on RPC (rpc-transport.md §3.3). `op_list.rs:27`'s "only
under `transport rpc`" goes away, and with it the parser rule at `model_attributes.rs:81`.

## 8. Clients

One contract in each language: **subscribe → `Ready` → items → end or error**, with the binding
(WebTransport, WS or SSE) chosen in client configuration, not in generated method names. The
default is `transport: "auto"`: try WebTransport, fall back to WS, then SSE. A WebTransport attempt
that has not reached `Ready{id: 0}` within 3 s counts as failed. The outcome is remembered per
origin for 10 minutes, wrapped in try/catch so it works without storage, so a client on a
UDP-blocked network pays that wait once, not on every subscription.

| | Rust (`cratestack-client-rust`) | TypeScript | Dart |
|---|---|---|---|
| Call | `client.widget().subscribe(input).await? -> impl Stream<Item = Result<ModelEvent<Widget>>>` | `client.widget.subscribe(input, { signal }): AsyncIterable<ModelEvent<Widget>>` | `client.widget.subscribe(input): Stream<ModelEvent<Widget>>` |
| Procedure | `client.room_messages(args)` | `client.roomMessages(args)` | `client.roomMessages(args)` |
| Ready | the `await` before the stream resolves on `Ready` | the iterator's first `next()` resolves only after `Ready` (plus an `onReady` option) | `Stream` `onListen` + `ready` future |
| WebTransport | `webtransport` feature: the §6a.5 crate's client natively, `web-sys` `WebTransport` on `wasm32` | browser `WebTransport`, one session per client, opened lazily, ref-counted, 5 s linger; not available on Node (falls back to WS) | Flutter native: the Rust client through `cratestack-client-flutter` (flutter_rust_bridge); Flutter web: JS interop to the browser API; pure Dart VM: falls back to WS |
| WS | `ws` feature: `tokio-tungstenite` natively, `web-sys` on `wasm32` | one shared socket per client, opened lazily, ref-counted, closed after the last unsubscribe plus a 5 s linger | `web_socket_channel` |
| SSE | `reqwest` byte stream (existing CBOR-seq decoder's sibling) | `fetch` + SSE parser (not `EventSource`: headers) | Dio response stream |
| Framework layer | — | swr `useSubscription`, react-query `useSubscription` + `useLiveQuery` (§2.1 loop, invalidates the query on each event or replaces in place by key) | riverpod `StreamProvider.autoDispose` |

TypeScript transports plug in as `RpcStreamLink` terminals, reusing the frame contract
streaming-rpc-links.md §4 already settled (`{kind: "output"} | {kind: "error"}`). Logger and auth
links therefore work on subscriptions without a third link type. `include_client_schema!` (the client
role) generates the Rust surface. `cratestack-client` gains the `ws` and `webtransport` features.
Neither pulls `axum`, so the `facade-disjointness` CI assertion is unchanged. `@datagram` ops
(§6a.7) surface as `LossySubscription<T>` in every language.

## 9. Server wiring and gating

- **Cargo:** a `ws` feature on `cratestack-axum` (enables `axum/ws`), forwarded by `cratestack-pg`
  and `cratestack-api`, and on `cratestack-client-rust`/`cratestack-client`. A `webtransport` feature
  on the same facades and client crates pulls in `cratestack-webtransport` (server) or its client
  half. Without it, no QUIC dependency enters the graph. SSE needs no feature — it ships today.
- **Schema:** `extension websocket { }` and `extension webtransport { }`, following extensions.md's
  three-layer rule: the schema declares participation, a same-named Cargo feature gates the code,
  and declaring one without the other is a compile error, never a silent no-op. Decision O4 is
  whether they should instead be generated whenever the feature is on. `@datagram` requires
  `extension webtransport`.
- **Backplane:** imperative, at router construction, like every other store:
  `rpc_router(…).with_backplane(PgNotifyBackplane::new(pool.clone()))`. Defaults are listed in §4.1;
  decision O6.
- **Disjointness:** `include_server_schema!(db = None)` gets subscription procedures and topics, with
  an in-process or Redis backplane, and no sqlx. Model feeds stay `db = Postgres`-only, because they
  need a model. The embedded and client macros emit no server subscription code.

## 10. Observability

One span per subscription (`cratestack_operation = "subscription"`, `cratestack_op_id`, principal,
binding: `sse` | `ws` | `webtransport`), closed with `delivered`, `dropped_by_policy`, and
`end_reason` set to one of `cancel`, `disconnect`, `lagged`, `resync`, `stream_end`, `error`,
`expired`. WebTransport sessions add QUIC statistics: RTT, lost packets, migrations, datagrams
dropped for size. The client reports which binding `auto` settled on, so operators can see how much
traffic actually falls back. The metrics are an active-subscription
gauge per op, lag and resync counters, refetch-query counts for class B, and backplane listener
reconnects. A lag counter that keeps rising is the operator's signal to raise the buffer or switch
to Redis. A migration count of zero on mobile-heavy traffic means the load balancer is not routing
by connection ID (§6a.6).

## 11. Slices

Every slice lands on REST and RPC together and on every binding it touches (CLAUDE.md, "Transport
parity"). Every slice that adds a `### ` changelog entry links its cratestack-docs and
cratestack-skills companions (CLAUDE.md, "Docs and skills parity").

| # | Slice | Depends on |
|---|---|---|
| S1 | **Access control.** Open-time policy (§5.1); class-A in-memory read-policy evaluator plus the differential test (§5.2); per-event filtering for class A. A class-B model with `@@subscribe` becomes a compile error until S3b, failing closed, a `fix!`. SSE keepalive. | — |
| S2 | **Backplane.** `SubscriptionBackplane` + in-process + `PgNotifyBackplane`; `pg_notify` in `enqueue_event_outbox`; `Resync`; `ready` event; `SKIP LOCKED` on the handler drain. Multi-replica integration test: two routers, one database, a write on A observed on B. | S1 |
| S3 | **Model feed input + REST.** `where`/`operations`/`fields`; `GET /<plural>/$subscribe`; lift the `transport rpc` requirement. | S2 |
| S3b | **Class-B refetch** with auth-equivalence grouping; tombstones per O2. | S3 |
| S4 | **Subscription procedures + topics.** Grammar, handler trait, SSE dispatch on both transports, `topic` codegen, `publish_in_tx`. | S2 |
| S5a | **Transport-neutral dispatcher** (§6.1): extract `subscription_dispatch` out of the axum handler; SSE becomes its first adapter, with no wire change. Byte-stream frame codec (CBOR-seq / NDJSON) shared by clients (§6a.3). | S3, S4 |
| S5 | **WebSocket binding** (subscriptions only): upgrade and `Init` auth, Origin, limits, keepalive, `extension websocket`. | S5a |
| S5b | **WebTransport binding.** Starts with a time-boxed interop spike: each O8 candidate against current Chrome, Firefox and Safari (Playwright drives all three) on draft negotiation, protocol negotiation, datagrams, migration. Then `cratestack-webtransport` (L4) with the `WtAcceptor` seam, session, control stream, stream-per-subscription adapter, CONNECT/`Init` auth, Origin, 0-RTT refusal, limits, `extension webtransport`, dev certificates via `serverCertificateHashes`. CI: a browser interop job, and a multi-replica test with a write on A observed over WebTransport on B. | S5a; independent of S5, so S5 and S5b can land in either order |
| S5c | **`@datagram` subscriptions** (§6a.7). | S5b |
| S6 | **Clients**: Rust, TS (links, swr, react-query), Dart (riverpod; Flutter native over the Rust client); `transport: "auto"` WT → WS → SSE; an `examples/realtime-chat` exercising topics, a model feed and all three bindings, including a forced-fallback test with UDP blocked; docs (including §6a.6's deployment requirements) and skills. | S5, S5b |
| S7 | Later, each gated on a concrete case: `RedisBackplane`, `@topic` shorthand, filter-exit events (O3), unary over WS/WT (§6.7). | — |

Because WebTransport is a production requirement, S5b may go ahead of S5. The only thing they share
is S5a. S6 is listed last for dependency order only.

**S5b status (as built, 2026-10-28).** Built:
- the spike (§6a.5);
- `cratestack-webtransport` with everything in S5b's row except the `extension webtransport`
  gating;
- `cratestack_core::SubscriptionDispatcher`, the S5a seam the binding drives;
- a native-client integration suite;
- the Chromium end-to-end check, `just test-webtransport-browser` (CI job `webtransport-browser`).

Not built: the multi-replica test (it needs S2's backplane). The `extension webtransport` gating
and the generated wiring landed with S5a (below). Each server slice's PR should also carry the
smallest client change that lets its integration test run through a generated client, not raw HTTP.

**S5a status (as built, 2026-10-31).** Built ahead of S3 and S4, for model feeds only (the one
subscription op that exists):
- **The generated dispatcher.** A `db = Postgres` server schema with a `@@subscribe` model gets a
  `subscriptions` module: `SubscriptionDispatch` implements `cratestack_core::SubscriptionDispatcher`,
  and `Cratestack::subscriptions()` returns one. Its `open` matches the op id and does what the SSE
  handler used to do inline: refuse a read policy it cannot decide on an event (§5.2's `@server_only`
  rule, cratestack#1253), register on the event bus, and filter every event through the subscriber's
  read policy. A model feed takes no input yet; anything but `null` or `{}` is `invalid_argument`
  until S3.
- **SSE as its first adapter, with no wire change.** The generated `GET /rpc/subscribe/{op_id}` arm
  checks `Accept`, authenticates, opens through the dispatcher and encodes the stream
  (`encode_subscription_sse_response`). The feed ends with an error item, which the adapter sends as
  the terminal `event: error`. Overflow is still `unavailable` / `"subscription lagged"`
  byte for byte (`rpc_subscribe_sse.rs` is unchanged and green). One difference: a read policy that
  cannot be decided for one event (a failed relation probe) now ends the stream with that error, not
  with `"subscription lagged"`.
- **`extension webtransport { }`**, §9's three layers: the schema declares it, the `webtransport`
  Cargo feature on `cratestack-pg` pulls in `cratestack-webtransport` (re-exported as
  `cratestack::webtransport`) and gates the codegen, and declaring one without the other is a compile
  error. The extension adds `SubscriptionDispatch::webtransport(config, auth)`, which binds the
  WebTransport server to the generated dispatcher. Declaring it in a schema with no subscription op is
  a compile error, as is declaring it under `include_embedded_schema!`. `cratestack-client`'s
  `webtransport` feature is a schema-compatibility switch only: the generated clients are S6.
- Tests: the dispatcher against Postgres (`rpc_subscribe_read_policy/dispatcher.rs`), and the dispatcher served over
  real QUIC against Postgres (`webtransport_model_feed.rs`, CI job `webtransport-browser`).

Not in S5a: the open-time refusal of §5.1 (a caller whose read policy is `False` before any row,
such as anonymous on `auth() != null`, still gets `Ready` and an empty feed, never an event), the
`ready` event and keepalive on SSE (S1/S2), and the byte-stream frame codec for clients, which
`CborSeqChunkDecoder`'s move to `cratestack-codec-cbor` in S5b already shares.

**S2 status (as built, 2026-11-01).**
- **The contract**, `cratestack_core::SubscriptionBackplane`: `publish`, and an async `subscribe`
  that resolves only once the subscription is live, so a binding that then sends `Ready` has missed
  nothing. `BackplaneMessage` has `Payload`, `Resync` (the listener reconnected) and `Lagged` (this
  receiver fell behind its 64-item buffer). The trait lives in `cratestack-core`. Both implementations
  live in `cratestack-sqlx`, because core has no async runtime (it also targets wasm):
  `InProcessBackplane` and `PgNotifyBackplane`. The Redis one is S7.
- **`PgNotifyBackplane` is the default (decision O6, as proposed).** One `PgListener` per process,
  opened by the first subscription, on the single Postgres channel `cratestack_events`. Backplane
  channels (`cratestack.model.<Model>`) are multiplexed inside the payload. A dropped listener connection
  sends `Resync` to every local subscription, which ends with `unavailable` / `"subscription
  interrupted"`. Swap it with `Cratestack::builder(pool).with_backplane(..)`: an `InProcessBackplane`
  for a single replica behind a pooler that drops `LISTEN`.
- **Model events are notified by the writing transaction.** `enqueue_event_outbox` sends
  `pg_notify` from the same statement as its `INSERT` (a data-modifying CTE), so Postgres delivers it
  only on commit, to every replica, in commit order. It does this only for `@@subscribe` models: the
  descriptor's new `subscribed` flag, read through `EventTarget`, so an `@@emit`-only model never
  takes Postgres' commit-time `NOTIFY` lock. An event over the 8000-byte limit goes as a reference to
  its outbox row, which the listener reads.
- **The drain claims rows with `FOR UPDATE SKIP LOCKED`** in one transaction. When the backplane is
  not notified in-transaction (in-process), the drain publishes each row's event after commit, on
  its first attempt only (at most once).
- **The generated dispatcher subscribes on the backplane** instead of registering event-bus
  handlers. `cratestack_axum::rpc::model_events` decodes the payloads into `ModelEvent<T>`.
- **SSE sends `event: ready`** (`{"id": 1, "ready": {}}`) first. Items follow from id 2.
- Tests: `rpc_subscribe_read_policy/replicas.rs` (two replicas, each with its own pool and listener:
  a write on A reaches an SSE subscriber on B, and the read policy still applies on B), and
  `webtransport_model_feed.rs` (the WebTransport server runs on replica B). The SSE overflow test pins
  the in-process backplane, which publishes synchronously from the drain.

**S1 status (as built, 2026-11-01).** Its per-event half shipped first, in cratestack#1253 and S5a.
The class-A/B split of §5.2 became one evaluator: the create path's, which runs row-local predicates
in memory and probes relations with `EXISTS`. That leaves no class-B refetch for S3b and no
seen-set tombstones (O2 moot): a delete event carries the deleted row and is decided like any other.
The rest:
- **Open-time refusal (§5.1)**: `cratestack_sqlx::refuse_unreadable_subscription`. Each predicate
  is reduced to the truth values it can take over all rows. A caller-only predicate is decided now.
  A column compared with an `auth()` claim the caller lacks is `FALSE` for every row, as the SQL
  path renders it. Any other column predicate can take any value. If no `@allow` can be `True`, or
  some `@deny` can never be `False`, the subscription is refused before `Ready`: `unauthenticated`
  for an anonymous caller, else `permission_denied`. Predicates are treated as independent, so it
  can open a feed that will deliver nothing; it never refuses one that could deliver something.
- **The SSE endpoint's errors are JSON with the error's own status** (`encode_subscribe_error`),
  `WWW-Authenticate` included. They used to go through the unary encoder's codec negotiation, which
  answered an `EventSource` client (`Accept: text/event-stream`) with `406` for every refusal.
- **SSE keepalive**: a `: keepalive` comment after 15 s without an item.
- **Credential expiry (§6.3)**: a subscription ends with `unauthenticated` / `"credential expired"`
  when the opening context's `exp` claim passes (`until_credential_expires`); the client resubscribes
  with a fresh credential. WebTransport re-authentication refreshes the context for streams opened
  after it, not for streams already open.

**S3 status (as built, 2026-11-01).** Model feeds take input, and REST serves them.
- **Input (§3.1)**: `list`'s own query syntax, parsed by the generated `list` parser
  (`parse_<model>_list_query`), so a filter means on a feed what it means on `list`: `field=value`,
  `field__op=value` (`contains`, `startsWith`, `in`, comparisons, `isNull`), `where=`, `or=`. Plus
  `operations=created,updated` and `fields=id,body`. On SSE (both transports) it is the query string;
  through the dispatcher (WebTransport) it is `RpcSubscribeInput`
  (`{"where", "or", "filters", "fields", "operations"}`), turned into that query string
  (`subscribe_input_query`), the same synthesis RPC's `list` uses.
- **Evaluated in memory** against the event's row (`cratestack_sql::filter_matches`), three-valued
  like SQL (a `NULL` comparison is not a match), before the read policy. It only narrows what the
  policy already allows.
- **Refused at open with `invalid_argument`**: `list`'s paging and shaping keys (`limit`, `offset`,
  `sort`, `orderBy`, `include`, `includeFields[...]`, `computedParams`), a filter the evaluator
  cannot decide in memory (a relation filter, full-text search), a filter over a column the event
  does not carry (`@server_only`), an unknown operation or an unknown field.
- **`fields`** projects the event's `data`. The read policy is evaluated on the whole row first.
- **REST**: `GET /<plural>/$subscribe` (`model_subscribe_route`), same dispatcher, same SSE wire.
  `@@subscribe` no longer requires `transport rpc`. Its route descriptor is
  `MODEL_<SNAKE>_SUBSCRIBE_GET`, so REST's op/route parity (`op_contract_parity`) covers it.
- **O3 decided: no filter-exit events.** A row updated out of a subscriber's filter produces no
  event for it, and the subscriber's copy goes stale. This is documented, and the §2.1 refetch loop
  is the remedy. The pre-image read stays `@@audit`-only.
- Tests: `rpc_subscribe_read_policy/filters.rs` (filter, projection, operations, on SSE and the
  dispatcher, and every refusal) and `rpc_subscribe_read_policy/rest.rs` (a REST schema's feed, its
  filter, its anonymous refusal and its paging refusal).

**S4 status (as built, 2026-11-01).** Subscription procedures and topics, on every binding.
- **Grammar (§3.2, §3.4)**: `subscription procedure name(args: T): Item` and
  `topic Name(key: String | Int | Uuid): Payload`. They live in IR lists of their own,
  `Schema::subscription_procedures` and `Schema::topics`. This is the `queries` precedent: no unary
  route, op descriptor or client stub iterates them by accident. The parser enforces:
  - a single item type;
  - `@stream`, `@isolation`, `@no_idempotency`, `@status`, `@no_rate_limit` and `@mcp` are refused,
    each with its reason;
  - `@allow`, `@deny`, `@authn`, `@authorize` and `@api_version` are kept;
  - a subscription procedure's name is unique among all procedures (same op id);
  - a topic key is one `String`, `Int` or `Uuid`.
- **Handler (§3.3)**: a `ProcedureRegistry` method returning
  `Future<Output = Result<SubscriptionStream<name::Output>, CratestackError>>`. The item type is
  the procedure's `Output`, since its module is a procedure's module. The design's `Item` alias was
  not added. The procedure's own `authorize_with_db` produces the `Authorized` witness: field
  validators, `@allow`/`@deny`, `@authn`, `@authorize`. A setup `Err` refuses before `Ready`. A
  stream that ends ends the subscription: SSE closes, WebTransport sends `end`. Unlike a model feed,
  this is not an error. A policy refusal of an anonymous caller is `403`, as for a unary procedure.
- **Input**: the procedure's `Args` object, the input a unary procedure takes over RPC.
  - **SSE `GET`** carries each argument as a query parameter whose value is that argument's JSON
    (`?args={"roomId":"r1"}`), so an `EventSource` can open it.
  - **`POST /rpc/subscribe/{op_id}`** (§7.1, now built, for model feeds too) takes the JSON input
    as its body.
  - **REST serves `GET <procedure path>`** (`/$procs/<name>`, `@api_version` applied) rather than
    §7.2's `POST`, because an `EventSource` can only `GET`. Its op key is `GET <path>`.
- **Dispatcher**: with subscription procedures it holds the registry. It is
  `SubscriptionDispatch<R>`, built by `db.subscriptions(registry)`. Without them it stays
  `db.subscriptions()`. REST's model `$subscribe` route opens its feed directly, since its router
  holds no registry.
- **Topics**: `db.topics().room_message(&room_id)` returns a `cratestack::Topic<ChatMessage>` on
  the runtime's backplane (channel `cratestack.topic.<Topic>.<key>`).
  - `publish(&msg)` publishes immediately.
  - `publish_in_tx(tx, &msg)` sends `pg_notify` inside the transaction. The message is delivered
    only if the transaction commits, in commit order with its model events. It is refused with a
    backplane the transaction cannot notify (`InProcessBackplane`).
  - `subscribe().await` resolves once the subscription is live. It ends `unavailable` on lag or
    resync.
  - A message must encode under Postgres' `NOTIFY` limit.
- **`db = None`**: subscription procedures work, with the handler's stream as the source. A topic is
  a compile error, because `cratestack-api` has no backplane yet; moving `InProcessBackplane` out of
  `cratestack-sqlx` is the follow-up.
- Not in S4:
  - `IsolatedCratestack` has no `topics()`. Inside an `@isolation` procedure, publish after commit.
  - The LSP does not index the new declarations.
  - Client methods come in S6.
- Tests: `cratestack-pg/tests/subscription_procedures.rs`. It covers:
  - RPC `GET`, `POST` and the dispatcher;
  - refusals by policy, by setup and for bad arguments;
  - key isolation;
  - `publish_in_tx` commit and rollback;
  - REST;
  - contract parity.

  `cratestack-api/tests/subscription_procedure_no_database.rs` covers `db = None` and a natural
  end. There are also parser, generator and runtime unit tests.

**S5 status (as built, 2026-11-01).** The WebSocket binding, `cratestack-websocket` (L4, axum `ws`).
- **Shared with WebTransport**: frames, admission (`Init`, `authenticate_session`, refusal close
  codes), `OriginPolicy` and the per-principal session registry moved into
  `cratestack_core::subscription_wire`. `cratestack-webtransport` re-exports them under its old
  names. `ClientFrame` gains `cancel` (`{id, cancel: {}}`), which is what WebSocket needs instead
  of WebTransport's stream reset.
- **Route (§6.1)**: `extension websocket { }` plus the facades' `websocket` feature (decision O4,
  as proposed). Declaring one without the other is a compile error. Both `cratestack-pg` and
  `cratestack-api` have the feature, so `db = None` subscription procedures can be served over
  WebSocket too. The extension adds `SubscriptionDispatch::websocket(config, auth)`, an
  `axum::Router` with the one route, `/rpc/ws` (`/$ws` on REST) unless `WebSocketConfig::path`
  says otherwise. The application merges it into its router.
- **Subprotocol (§6.2)**: the first offered of `cratestack.rpc.v1+cbor` / `+json`, else the
  `codec` query parameter, else CBOR (406 for an unknown one). Binary messages carry CBOR and text
  messages carry JSON; a message of the wrong kind closes with 4400.
- **Authentication (§6.3)**: the first frame must be `Init` within 10 s. It is always required, as
  on WebTransport: a native client sends its headers on the upgrade and an empty `Init`; a browser
  puts them in `Init`. The handling of `Init` and its refusals follows WebTransport:
  - a refusal is `{id: 0, err}`, then close 4401/4403/4400/4500;
  - re-authentication with a further `Init` must name the same principal (else 4403);
  - the socket closes 4401 when the context's `exp` passes without a refresh.
- **Origin**: `SameOrigin` by default. Because an upgrade can arrive over plain HTTP behind a
  TLS-terminating proxy, the authority's `http` origin is accepted too
  (`OriginPolicy::permits_any_scheme`). An origin that is not allowed gets 403 on the upgrade.
- **Frames (§6.4)**:
  - a `Request` id already in use, or id 0, gets `Error{invalid_argument}`;
  - past the subscription limit, a `Request` gets `Error{resource_exhausted}`;
  - an unknown op gets the dispatcher's `not_found`;
  - `Cancel` gets `end`;
  - a stream that ends gets `end`.
- **Keepalive and limits (§6.5, §6.6)**, all in `WebSocketLimits`:
  - a ping every 15 s, and close 1001 after 30 s without a pong;
  - 64 subscriptions per socket and 8 sockets per principal (close 4429);
  - the body limit per message (1009, from tungstenite);
  - a 1024-frame outbound queue, which closes the socket 1013 when full.
- **O5 decided: unary over WebSocket stays gated** (§6.7). The dispatcher only knows subscription
  ops, so any other op is `not_found`.
- Tests:
  - `cratestack-websocket/tests/session.rs` uses a real socket: multiplexing, id reuse, `Cancel`,
    the 4401 refusal, JSON over text, and `Origin`;
  - `cratestack-pg/tests/websocket_subscriptions.rs` runs a model feed and a subscription procedure
    on one socket against Postgres, in CI's db job.

**S6 status, Rust (as built, 2026-11-01).** `cratestack-client-rust` subscribes.
- **The call** is `subscribe(&RpcSubscribeInput)` on a model client and `<procedure>(&Args)` on
  `ProceduresClient`, generated for REST and RPC clients and for `include_client_schema!`. It
  resolves at `Ready` to a `ClientSubscription<Item>`, a bounded `mpsc::Receiver<Result<Item,
  ClientError>>`. A model feed's item is `ModelEvent<Model>`. With `fields` it is partial, so read
  it as `ModelEvent<serde_json::Value>` through `runtime().subscribe`.
- **Bindings** (`ClientConfig::subscription_transport`):
  - **SSE** is always `GET`, with the input as the query string: a model feed's `list` filters, a
    procedure's JSON-valued arguments. It goes through `request_streamed_with_query_and_accept`,
    so the request authorizer signs it like any call. A JSON `POST` body would not match the
    authorizer's canonical request, which names the codec's content type.
  - **WebSocket** (`ws` feature, native targets: `tokio-tungstenite`, rustls with native roots)
    uses one socket per subscription. The upgrade carries the authorizer's headers for
    `GET <socket path>`, and the socket sends an empty `Init` and a `Request`. Dropping the
    receiver sends `Cancel`. A shared, multiplexed socket is a later optimisation; per-subscription
    sockets keep the client simple.
  - **WebTransport** (`webtransport` feature, native targets) uses one session per subscription,
    through `cratestack-webtransport`'s client half (below). `ClientConfig::with_webtransport(url,
    trust)` names the listener, a separate UDP address.
  - **`Auto`** tries WebTransport (when configured), then the socket, then SSE. It moves on only
    when a binding cannot be opened (`Unreachable`, as opposed to `Refused`). A WebTransport
    session not open within 3 s counts as unreachable, and the failure is remembered for 10
    minutes across clones of the config, as in the TypeScript client.
- **The client half of `cratestack-webtransport`** (feature `client`, as built 2026-11-01).
  - `WebTransportClient::connect(url, trust, headers)` CONNECTs, offers the CBOR subprotocol, sends
    `Init` with the headers, and resolves at `Ready{0}`. The generated client fills `headers` from
    its request authorizer for `CONNECT <path>`, the request the server authenticates (§6a.4), so a
    signing authorizer signs what is verified. Frames up to `DEFAULT_BODY_LIMIT_BYTES` are accepted,
    the server's own default inbound limit. It keeps the control stream open, because
    the server ends a session whose control stream ends.
  - `subscribe(op, input)` opens one bidirectional stream, writes the `Request`, finishes the send
    half and resolves at `Ready{id}`. Dropping the `Subscription` stops the stream, which is the
    cancel (§6a.2).
  - `ServerTrust` is `NativeRoots`, or `CertificateHashes` for a `DevCertificate`. The pinned
    verifier still checks the handshake signature.
  - The endpoint binds the address family of the server: `web-transport-quinn`'s builder binds
    `[::]:0`, which fails on hosts without IPv6. 0-RTT is off, as on the server.
  - Failures are `Unreachable` (network, UDP, certificate) or `Refused` (an `err` frame, or a
    close code with no frame: 4401 is `unauthenticated`, 4403 `permission_denied`, 4429
    `resource_exhausted`).
- Tests:
  - `cratestack-webtransport/tests/client.rs` runs the client half against the real server: two
    subscriptions on one session, the clean end, refusals at `Init` and `Request`, a terminal
    error, the cancel reaching the dispatcher, and a wrong pin or an unrooted certificate being
    unreachable;
  - `examples/realtime-chat/tests/webtransport.rs` runs the generated Rust client against the
    example's QUIC listener and Postgres: pinned to WebTransport and on `Auto`, with no HTTP server
    reachable, so only WebTransport could deliver; and `Auto` with UDP blocked falling back to the
    WebSocket within the 3 s bound, then not waiting again;
  - `cratestack-pg/tests/websocket_subscriptions.rs` runs the generated client over each binding
    against Postgres, model feed and procedure;
  - `cratestack-client/tests/subscriptions.rs` covers the client role's surface;
  - there are SSE parser units.

**S6 status, TypeScript (as built, 2026-11-01).** The generated package subscribes.
- **Files**: `src/subscriptions.ts` (the API, `"auto"`, SSE) and `src/subscription-sockets.ts`
  (WebTransport, WebSocket). They are emitted for REST and RPC packages only when the schema has a
  subscription op, so every other package, and every committed snapshot, is unchanged. The runtime
  gains `subscriptions?: SubscriptionClientOptions` under the same condition.
- **The call** is `client.<model>.subscribe(input, { signal, onReady, headers })` and
  `client.procedures.<name>(args, options)`. It returns `AsyncIterable<Item>`; the first `next()`
  resolves after `Ready`. Items are revived like unary responses. A model event's `operation` is
  `"Created" | "Updated" | "Deleted"`, the server's spelling. Errors are
  `CratestackSubscriptionError { code, message, details, status }`.
- **Bindings**, all with JSON frames:
  - **WebTransport** uses one session per URL and credentials, `?codec=json`, an `Init` with the
    client's headers, and one bidirectional stream per subscription (the `Request` is written,
    then the writer is closed; cancel is `reader.cancel()`). The URL is client configuration
    (`webTransport.url`), because the listener is a separate UDP address.
  - **WebSocket** uses one shared socket per URL and credentials, multiplexed by id, with `cancel`
    and a 5 s linger.
  - **SSE** is `fetch` with a stream parser, not `EventSource`, because it needs headers.
- **`"auto"`** tries WebTransport (only with `webTransport.url` and a platform `WebTransport`), then
  WebSocket, then SSE. A transport that never opens raises `SocketUnreachable` and falls back. A
  refusal is thrown. A WebTransport attempt not `Ready{0}` within 3 s is remembered in
  `localStorage` (inside a try/catch) for 10 minutes per origin. Node has no `WebTransport`, so
  there it starts at the WebSocket (Node 22's global) or SSE.
- **Framework layer**:
  - `--swr` gets a generic `useSubscription(key, open)` over `swr/subscription`.
  - `--tanstack` gets `useSubscriptionQuery(key, open, { onItem, queryOptions })`, built from
    `useQuery` and `useQueryClient` only, since that layout declares no `react` dependency. Its
    `subscriptionQueryOptions` writes each item with `setQueryData` rather than using TanStack's
    `streamedQuery`, which is still experimental and changed shape within 5.x, beyond the
    package's `^5.0.0` floor. The query is pending until the first item, then holds the latest
    one. Its last observer leaving cancels the query, which aborts the subscription. To keep a
    list fresh, invalidate its key in `onItem`.
- Tests:
  - `tests/subscriptions_runtime.rs` runs vitest against an SSE `fetch` and a fake socket: Ready
    gating, revival, terminal error, multiplexing and cancel, `"auto"`'s fallback and its refusal;
    and `subscriptionQueryOptions` under a real `QueryClient` (items in the cache, abort on the
    last observer leaving);
  - `just verify-typescript` builds the subscription fixture with `--swr --tanstack` through
    `tsc`;
  - `tests/browser/webtransport.cjs` runs the generated modules in headless Chromium against
    `browser_harness`: items, credentials through `Init`, refusal, cancel, and a second credential
    getting its own refused session. `just test-ts-webtransport-browser` runs it, in CI's
    WebTransport job.

**S6 status, Dart (as built, 2026-11-01).** SSE through the HTTP seam.
- **The call**: `client.<model>.subscribe(input: CratestackSubscribeInput(...), headers, onReady)`
  returns `Stream<CratestackModelEvent<Model>>`; `client.procedures.<name>(args, ...)` returns
  `Stream<Item>`. Items decode like unary results. A refusal or terminal error is a
  `CratestackSubscriptionException`. Cancelling the listen completes the request's `abort`.
- **The binding**: SSE only, over `CratestackHttpTransport` (`cratestackSubscribeSse` in the
  runtime). It therefore runs on every transport an app already uses, the Rust transport included
  (which is how Flutter native reaches the Rust client). It needs the generated HTTP adapter
  (`CratestackHttpAdapter` / `CratestackRpcHttpAdapter`); any other adapter gets an
  `UnsupportedError` stream.
- **riverpod** (`--preset riverpod`, and fespalier, which shares its builders): the API files carry
  the same methods, and each op gets a `@riverpod` stream provider, `<model>Events(input:)` and
  `<procedure>(args)`, named after the five model providers and every unary procedure provider, so
  no existing name moves. `CratestackSubscribeInput` compares by its query string, so a family
  keyed by an equal input reuses its subscription.
- **WebSocket and WebTransport** (as built 2026-11-01). A schema with a subscription op adds
  `lib/src/subscription_sockets.dart`, plus a WebTransport module that a conditional import picks:
  `subscription_webtransport_web.dart` (`package:web`, the browser's `WebTransport`) when
  `dart.library.js_interop` exists, a stub elsewhere. The package gains `web_socket_channel` and
  `web`; every other package is unchanged.
  - **Options**: `CratestackSubscriptionOptions { transport, webTransportUrl,
    serverCertificateHashes, webSocketUrl, credentials }` is passed to the generated client's
    constructor (`subscriptions:`), or overrides `<prefix>SubscriptionOptionsProvider` in the
    riverpod preset.
  - **Frames**: JSON on both sockets. WebSocket uses the `cratestack.rpc.v1+json` subprotocol and
    text frames. WebTransport uses `?codec=json` and one document per line. Each opens one socket
    or session per subscription: `Init` with `credentials`, one `Request`, `Ready`, items, `end` or
    `err`. A WebSocket cancel sends `{id, cancel}`. A WebTransport cancel stops reading and closes
    the session. On WebTransport the control stream stays open, because the server ends a session
    whose control stream ends.
  - **`auto`**: WebTransport (web only, with `webTransportUrl`), then the WebSocket, then SSE. It
    moves on only when a binding cannot be opened. Like the TypeScript client, it has a 3 s
    WebTransport bound and remembers a failure for 10 minutes per origin.
  - **Credentials**: a socket does not go through `CratestackHttpTransport`, so it cannot carry
    what that transport adds (a Dio interceptor's token, fespalier's session). `credentials`
    supplies `Init`'s headers. Until it is set, `auto` stays on SSE, because a socket would
    otherwise connect anonymously and be refused, and a refusal never falls back. A pinned
    `webSocket` or `webTransport` binding uses whatever is given.
  - Native Dart has no WebTransport, so there `auto` starts at the WebSocket. Flutter native can
    still reach WebTransport through the Rust transport.
- The SSE reader now normalises `\r\n` after joining chunks, so a CRLF split across two chunks
  still ends its event, the same fix as the Rust client's.
- Tests: `tests/subscriptions.rs` and `tests/subscriptions_riverpod.rs` pin the generated surface
  (and its absence elsewhere). `subscriptions_rpc` is also a riverpod fixture of `just verify-dart`:
  `build_runner`, `flutter analyze --fatal-warnings`, and `subscriptions_rpc_riverpod_test.dart`
  reading both stream providers through a `ProviderContainer`.
  `fixtures/subscriptions_rpc_test.dart` runs under `just verify-dart` (CI's Dart job) against an
  SSE-answering transport: Ready, items, the request, the refusal and the terminal error.
  `fixtures/subscriptions_rpc_socket_test.dart` runs there too, against a `dart:io` WebSocket
  server in the test process. It covers:
  - the pinned socket, with the subprotocol, `Init`'s credentials, the request input, the items and
    the end;
  - `auto` taking the socket, with cancel sending `cancel`;
  - `auto` falling back to SSE when no socket opens, across a split CRLF;
  - a socket refusal thrown with no SSE request made;
  - no socket attempted without credentials.
  `tests/browser/webtransport_entry.dart`, compiled to JavaScript, runs in Chromium against
  `browser_harness` (`just test-dart-webtransport-browser`, CI's WebTransport job). It covers:
  - items after `Ready`;
  - credentials reaching the `AuthProvider` through `Init`;
  - refused opens and sessions;
  - cancel;
  - `auto` taking WebTransport with no HTTP request made;
  - `auto` moving on within its bound when UDP is blocked.

**S6 status, the example (as built, 2026-11-01).** `examples/realtime-chat`.
- **What it has**:
  - messages (`@@subscribe`);
  - `RoomTyping`, a topic behind the `typing` subscription procedure;
  - `sendTyping`, a unary mutation that publishes.
- **Where it is served**:
  - unary calls, SSE and the WebSocket on the HTTP listener, with JSON on the wire;
  - WebTransport on a UDP listener with a `DevCertificate`. Its hash and both addresses are printed
    as one JSON line on startup.
- **`just test-realtime-chat`** (CI's WebTransport job, Postgres in a container):
  - `tests/chat.rs` runs the generated Rust client:
    - the model feed over the WebSocket;
    - the topic over SSE, published by the unary call;
    - `Auto` falling back to SSE when the WebSocket route is absent.
  - `tests/browser/fallback.cjs` runs the generated TypeScript client's `"auto"` in Chromium:
    - UDP open: WebTransport, with no WebSocket and no SSE request seen;
    - UDP blocked (the URL names a dead UDP port): the WebSocket;
    - SSE when chosen.

## 12. Decisions for the maintainer

- **O1. Is this the "new kind"?** The proposal: subscription procedures + topics (§3.2–3.4). The
  narrower alternative is filtered model feeds only (§3.1), which is S1–S3 alone.
- **O2. Class-B deletes**: seen-set tombstones (proposed), or "class-B models do not deliver deletes"
  (simpler, and a surprising gap).
- **O3. Filter-exit events**: add `matches: false` updates from a pre-image read, or document the
  staleness and leave it to the §2.1 refetch loop. **Decided (S3): document the staleness**;
  filter-exit events stay in S7.
- **O4. `extension websocket { }`** (proposed, matches extensions.md) or feature-only.
  **Decided (S5): the extension, as proposed.**
- **O5. Unary over WS**: keep gated (proposed) or schedule with S5. **Decided (S5): gated.**
- **O6. Backplane default under `cratestack-pg`** — *decided in S2: `PgNotifyBackplane`, swappable
  with `with_backplane`*: `PgNotifyBackplane` automatically (proposed; it
  has a pool, and `cratestack-sync` already listens), or in-process until configured (safer for
  single-replica apps behind PgBouncer in transaction mode, where `LISTEN` does not work — the
  listener needs a direct connection, configurable separately).
- **O7. Browser auth**: the `Init` frame (proposed) versus short-lived signed upgrade tickets
  (`POST /rpc/ws-ticket` → one-use token in the URL), which some deployments prefer for
  load-balancer-level auth. The answer applies to WS and WebTransport alike.
- **O8. WebTransport implementation**: **decided for Chromium, `web-transport-quinn`** (§6a.5's
  table). Still open: confirm on Firefox and Safari before relying on them in production.
- **O9. `@datagram`** (§6a.7): ship with S5b, or keep it for when the app has a lossy use case.
- **O10. Load balancer target**: document QUIC-LB connection-ID routing as required (sessions survive
  network changes) or as recommended (4-tuple routing works, and a network change becomes a clean
  resubscribe).

## 13. Alternatives considered

- **SSE only, never WS.** rpc-transport.md §3.4a's argument holds under HTTP/2. This design builds WS
  for what SSE cannot do: many subscriptions on HTTP/1.1 (the six-connection cap), one radio-friendly
  socket on mobile, client-to-server control (`Cancel`, credential refresh) without a new request,
  and binary CBOR frames.
- **WebTransport only, no WS.** Rejected: networks that block UDP 443 and older Safari would have no
  binding at all apart from SSE. WS is the fallback that keeps one multiplexed socket for those
  clients.
- **WebTransport through an HTTP/3-capable reverse proxy instead of in-process.** Would keep a single
  listener in the app, but no common proxy forwards WebTransport today (§6a.6). The in-process
  listener behind a UDP L4 balancer is what works now. If proxy support arrives, the binding is
  unchanged: only the listener moves.
- **Map subscriptions onto WebTransport unidirectional streams.** A server-opened unidirectional
  stream per subscription saves the client's half of the stream, but leaves no place for the
  `Request`, and cancel needs a second signal. A client-opened bidirectional stream carries the
  request, the response and the cancel on one stream.
- **Adopt the `graphql-ws` or tRPC protocol.** A ready-made client ecosystem, but they bring a
  second envelope next to §2.3's, which every CrateStack client already speaks for batch and stream.
  The `Init`/`Ready` handshake borrows `graphql-ws`'s `connection_init`/`connection_ack` idea without
  its envelope.
- **CDC / logical replication as the event source.** Rejected for the same reasons as
  sync-engine.md's table: infrastructure the framework cannot assume, and it replicates storage rows,
  not policy-filtered views.
- **A `LISTEN` per subscriber.** One database connection per open subscription. §4.2.
- **Make subscriptions resumable** (`Last-Event-ID`, outbox replay). The outbox could support it
  (`event_id` is a UUID, rows are retained), but resumable, ordered, policy-correct delivery across
  disconnects is what `cratestack-sync` exists for. Two half-overlapping mechanisms would be worse
  than a clear line: a **subscription** tells you what is happening now, and **sync** gives you a
  correct local copy.
- **Keep row policy unenforced on subscriptions** (rpc-transport.md §3.4a's first cut). Rejected for
  anything beyond that first cut: a feed that shows rows `get` refuses is a data leak with extra
  steps, and an in-memory evaluator plus refetch closes it at bounded cost (§5.2).
