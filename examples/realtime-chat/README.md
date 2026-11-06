# realtime-chat

A chat that is live over every subscription binding (slice S6,
[`docs/design/subscriptions-websocket.md`](../../docs/design/subscriptions-websocket.md)):

- **Messages** are a model feed. `model.Message.subscribe` delivers each created
  message that passes `Message`'s read policy.
- **Typing indicators** are a topic, `RoomTyping`, which is never stored. The
  `typing` subscription procedure checks its policy and then subscribes to it;
  `sendTyping` publishes to it.

The server serves both over three bindings:

| Binding | Where | When a client uses it |
| --- | --- | --- |
| WebTransport | `https://127.0.0.1:4433/rpc/wt` (UDP) | first, when the client knows the URL |
| WebSocket | `ws://127.0.0.1:8080/rpc/ws` | when UDP is blocked |
| SSE | `GET /rpc/subscribe/<op>` | when no socket opens |

The generated clients choose with `"auto"` (TypeScript) or
`SubscriptionTransport::Auto` (Rust). The Rust client tries WebTransport when its
config names the listener (`ClientConfig::with_webtransport`, feature
`client-webtransport`), and starts at the WebSocket otherwise.

## Run it

```bash
DATABASE_URL=postgres://... cargo run -p realtime-chat-example
```

On startup the server prints one JSON line:

```json
{"http":"http://127.0.0.1:8080","webtransport":"https://127.0.0.1:4433/rpc/wt","sha256":[...]}
```

WebTransport runs on a dev certificate made at startup (`DevCertificate`). It is
self-signed and valid for at most 14 days, which is what a browser accepts through
`serverCertificateHashes`. `sha256` is its hash. Pass it to the TypeScript client:

```ts
new Client(origin, {
  subscriptions: {
    transport: "auto",
    webTransport: {
      url: started.webtransport,
      serverCertificateHashes: [{ algorithm: "sha-256", value: new Uint8Array(started.sha256) }],
    },
  },
});
```

In production, serve a real certificate instead, and see §6a.6 of the design for
what a deployment needs (a UDP load balancer, the WebSocket fallback).

Environment variables:

- `REALTIME_CHAT_HTTP` (default `127.0.0.1:8080`);
- `REALTIME_CHAT_WT` (UDP, default `127.0.0.1:4433`);
- `REALTIME_CHAT_STATIC`, a directory to serve at `/` for a web client.

## Test it

```bash
just test-realtime-chat
```

The recipe uses `DATABASE_URL`, or starts a throwaway Postgres container. It runs
two sets of tests:

- `tests/chat.rs`: the generated Rust client, with messages over the WebSocket and
  typing over SSE, published by a unary `sendTyping`. It also checks that `Auto`
  falls back to SSE behind a proxy that drops upgrades.
- `tests/webtransport.rs`: the generated Rust client over WebTransport, pinned to the
  dev certificate. It delivers with no HTTP server reachable, and with UDP blocked
  `Auto` falls back to the WebSocket within 3 s, then does not wait again.
- `tests/browser/fallback.cjs`: the generated TypeScript client in headless
  Chromium, on `"auto"`:
  - with UDP open, it delivers over WebTransport and opens neither a WebSocket nor
    SSE;
  - with UDP blocked, it falls back to the WebSocket;
  - SSE works when chosen.
