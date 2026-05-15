# rpc-streaming-client-rust-example

Rust binary that consumes an `application/cbor-seq` stream from a `transport rpc` server via `RpcClient::call_streaming` from `cratestack-client-rust`. Companion to [`rpc-streaming-example`](../rpc-streaming/), which serves the other side of the wire.

## What it shows

- **Client-side streaming.** `RpcClient::call_streaming::<I, O>(op_id, &input)` returns `Result<mpsc::Receiver<Result<O, RpcClientError>>, RpcClientError>`. Items arrive on the channel as cbor-seq frames parse off the wire — no full-body buffering, time-to-first-item is the time to one chunk.
- **The error split.** Non-2xx responses (auth failure, unknown op, server error) surface from the outer `call_streaming(...)` `Result` before the channel ever opens. Per-item failures (mid-stream decode error, transport reset) appear as terminal `Err` items on the channel, after which `recv()` returns `None`.
- **op_id format.** Procedures map to `procedure.<raw schema name>`; models map to `model.<ModelName>.<verb>`. The server's URL is always `POST /rpc/{op_id}`.
- **Auth via `RequestAuthorizer`.** `RpcClient::new` wraps a configured `CratestackClient`, so any `RequestAuthorizer` set on the inner client (header injection, request signing, JWT, HMAC envelope) flows through to streaming calls automatically.

## Run

Two terminals:

```bash
# Terminal 1 — start the server example:
cargo run -p rpc-streaming-example

# Terminal 2 — consume the stream:
REMOTE_URL=http://localhost:3001 cargo run -p rpc-streaming-client-rust-example
```

Expected output (Terminal 2):

```
Streaming `procedure.ticks` from http://localhost:3001/ (start=100, count=10):

  index=0    value=100
  index=1    value=101
  …
  index=9    value=109

stream closed cleanly after 10 items
```

## Tests

`tests/smoke.rs` spawns a tiny in-process axum server that emits cbor-seq frames one-at-a-time and exercises:

- `streams_each_tick_as_it_arrives` — all items arrive in order; channel closes cleanly.
- `missing_auth_header_surfaces_as_remote_error_before_stream_opens` — 401 from the server surfaces as `Err` from `call_streaming` before any channel is opened.

Run with `cargo test -p rpc-streaming-client-rust-example`. Self-contained — does not require the server example to be running.

## See Also

- [`examples/rpc-streaming/`](../rpc-streaming/) — server side of the same op
- [`examples/rpc-procedures/`](../rpc-procedures/) — unary RPC procedures (no streaming)
- [`examples/rpc-batch/`](../rpc-batch/) — batching unary calls via `POST /rpc/batch`
- [`crates/cratestack-client-rust/src/lib.rs`](../../crates/cratestack-client-rust/src/lib.rs) — `RpcClient` source

## Limitations

This example uses `RpcClient` directly with locally-defined `TickerInput` / `Tick` types. `cratestack`'s `include_client_schema!` macro currently still emits REST-shaped client wrappers regardless of `transport rpc` — wiring it to also emit `RpcClient`-shaped typed methods is a follow-up. For now, schemas with `transport rpc` consume their server via either:

- the macro-generated typed REST surface (if the server also serves REST routes), or
- this example's pattern: hand-define input/output structs that match `type` declarations in the schema, then call `RpcClient::call_streaming(op_id, &input)`.
