# cratestack-websocket

The **WebSocket subscription binding**: subscriptions multiplexed over one WebSocket.
Design: [`docs/design/subscriptions-websocket.md`][design] §6.

```text
client                                              server
  │  GET /rpc/ws  Upgrade  (protocol cratestack.rpc.v1+cbor | +json)
  │  Init ─▶  ◀─ Ready{0}                 auth: upgrade headers, or Init's for a browser
  │  Request{id: n} ─▶  ◀─ Ready{n}  ◀─ next …  ◀─ end | err
  │  Cancel{id: n} ─▶  ◀─ end{n}
  │  Init ─▶                              refresh, same principal only
```

It is the fallback where UDP is blocked and the WebTransport binding cannot run: the same
frames, the same `Init`/`Ready` handshake, the same delivery contract. Binary messages carry
CBOR and text messages JSON, so `Decimal`, `BigInt` and bytes survive, unlike over SSE.

A schema serves it with `extension websocket { }` and the facade's `websocket` feature:

```rust
let app = router.merge(db.subscriptions().websocket(WebSocketConfig::new(), auth));
```

Limits (`WebSocketLimits`): 64 subscriptions per socket (`resource_exhausted` past it),
8 sockets per principal (close 4429), the request body limit per message (close 1009), a
1024-frame outbound queue (close 1013), a ping every 15 s with 30 s to answer. A refusal sends
`{id: 0, err}` before closing with 4401, 4403, 4400 or 4500.

Rate limiting is opt-in: `WebSocketConfig::new().rate_limit(layer.for_subscriptions())`, with
`layer` the application's `cratestack_axum::ratelimit::RateLimitLayer`, charges each `Request` one
token in the HTTP layer's store (bucket `princ:<hash>`, else `ip:<peer>`). A throttled open is an
`err` frame on that id, before `Ready`: `resource_exhausted` with `details.retry_after_secs`, and
the socket stays open. Serve with `into_make_service_with_connect_info::<SocketAddr>()`, or an
anonymous socket (no principal, no peer) is refused `failed_precondition`.

[design]: https://github.com/cratestack/cratestack/blob/main/docs/design/subscriptions-websocket.md
