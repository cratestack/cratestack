# cratestack-webtransport

The **WebTransport subscription binding**: subscriptions over an HTTP/3 WebTransport session,
one QUIC stream per subscription. Design: [`docs/design/subscriptions-websocket.md`][design] §6a.

```text
client                                              server
  │  CONNECT /rpc/wt   (protocol cratestack.rpc.v1+cbor | +json)
  │  1st bidi stream (control):  Init ─▶  ◀─ Ready{0}         auth, later re-auth
  │  each later bidi stream = one subscription:
  │     Request ─▶  ◀─ Ready{n}  ◀─ next …  ◀─ end | err, FIN
  │     client stops reading / resets  =  cancel
```

Why a separate binding from SSE and WebSocket: streams don't hold each other up (no
head-of-line blocking across subscriptions), a subscription survives the client changing
networks (QUIC connection migration), and cancel is a stream reset. The frames, the
`Init`/`Ready` handshake and the delivery contract are the same on every binding.

## Status

The binding runtime: session admission, authentication, the stream-per-subscription adapter,
limits and the frame codec. It is driven through `cratestack_core::SubscriptionDispatcher`, which
the schema macro implements once for every binding (design §11 S5a): a schema with `@@subscribe`
models gets `Cratestack::subscriptions()`. You can also implement the trait by hand. Datagram
subscriptions (`@datagram`, S5c) and the generated clients (S6) are later slices.

## Use

Declare the extension in the schema, and enable the same-named feature on the facade:

```text
transport rpc

extension webtransport {
}
```

```toml
cratestack = { package = "cratestack-pg", version = "0.17", features = ["webtransport"] }
```

```rust,ignore
use cratestack::webtransport::{OriginPolicy, WebTransportConfig};

let config = WebTransportConfig::new("[::]:443".parse()?, certificate_chain, private_key)
    .origins(OriginPolicy::Allow(vec!["https://app.example.com".into()]));
// The same AuthProvider your axum router uses.
let server = db.subscriptions().webtransport(config, auth_provider)?;
tokio::spawn(server.serve_with_shutdown(shutdown_signal));
```

Every `@@subscribe` model is then served at `model.<Model>.subscribe`, with the same per-event
read policy as the SSE endpoint. Without the extension, `WebTransportServer::bind(config, auth,
db.subscriptions())` does the same from a direct dependency on this crate.

- **Path**: `/rpc/wt` by default; `/$wt` for REST schemas (`.path("/$wt")`). Anything else is
  refused with 404.
- **Codec**: negotiated through the WebTransport subprotocol (`cratestack.rpc.v1+cbor` /
  `+json`), else the `?codec=cbor|json` query parameter, else CBOR. CBOR frames are a CBOR
  sequence; JSON frames are one per line.
- **Authentication**: the first stream must carry `Init` within `init_timeout` (10 s). Its
  `headers` are handed to your `AuthProvider` as if they were CONNECT headers; when it is empty,
  the CONNECT request's own headers are used (native clients). A later `Init` re-authenticates
  and must name the same principal (`PrincipalKeyFn`, default `actor.id` → `id` → `sub`).
- **Origin**: a request carrying `Origin` must match the policy (default: same origin). A
  request without one is not from a browser page.
- **Limits** (`WebTransportLimits`): 64 subscriptions per session (past it: `resource_exhausted`
  on that request), 8 sessions per principal (anonymous sessions counted per IP; past it: close
  4429), 2 MiB frames, 15 s keepalive, 30 s idle timeout. 0-RTT is always refused.

**Refusals** (ADR 0021 §1): before closing, the server sends `{id: 0, err: RpcErrorBody}` on the
control stream, carrying an `AuthRefusal`'s `details` and extra members (not its `WWW-Authenticate`
challenges: a frame has no headers). Close codes: 4401 a 401 refusal (or no `Init` in time, with no
frame), 4403 a 403 refusal or a re-authentication naming another principal, 4400 a protocol error or
malformed `Init` header, 4429 too many sessions, 4500 the provider could not decide (retry; not a
verdict on the credentials). Stream reset codes: `0x01` going away, `0x02` protocol.

## Deploying

It is a second listener, on UDP. Read design §6a.6 before production: an L4 UDP load
balancer, connection-ID routing if sessions should survive network changes, client IPs from the
QUIC peer address, and a WebSocket/SSE fallback for networks that block UDP.

For local development, the `dev-cert` feature's `DevCertificate::generate` makes a
short-lived self-signed certificate and the SHA-256 hash a page passes as
`serverCertificateHashes`.

## Tests

- `cargo test -p cratestack-webtransport`: codec and admission units, and native-client
  integration tests over real QUIC.
- `just test-webtransport-browser`: headless Chromium end to end (CI job
  `webtransport-browser`).

The QUIC/WebTransport implementation is [`web-transport-quinn`][wtq], reached only through
`src/transport/`. It was chosen over `wtransport` by an interop spike against Chromium
(design §6a.5, decision O8).

[design]: ../../docs/design/subscriptions-websocket.md
[wtq]: https://crates.io/crates/web-transport-quinn
