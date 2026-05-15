# rpc-streaming example

RPC server with a list-return procedure (`ticks`) streamed via
`Accept: application/cbor-seq`. The proof point: **streaming is content
negotiation**, not a different route. The same `POST /rpc/procedure.ticks`
serves a single CBOR `Vec<Tick>` or a stream of CBOR chunks depending on
what the client asks for.

## What you'll see

- A procedure with `T[]` return type — the macro emits `OpKind::Sequence`
  for it via the existing `TypeArity::List` branch.
- One URL, two wire shapes: default Accept → single `Vec`, `Accept:
  application/cbor-seq` → streamed chunks.
- The framework's existing sequence encoder
  (`encode_transport_sequence_result_with_status_for`) does the work; the
  RPC dispatcher delegates unchanged.

## Run the server

```bash
cargo run -p rpc-streaming-example
```

The server binds on `127.0.0.1:3001` and logs example curl commands.

## Read the smoke tests

`tests/smoke.rs` is the demo. Each test exercises the same op with a
different Accept and checks the response shape:

- `ticks_returns_single_cbor_vec_with_default_accept` — default content
  negotiation yields a single CBOR `Vec<Tick>` body.
- `ticks_streams_cbor_seq_when_negotiated` — same URL, same body,
  different Accept → streamed cbor-seq chunks. The example crate exports
  a `decode_cbor_seq` helper.
- `zero_count_returns_empty_sequence` — empty `Sequence` ops produce zero
  chunks + a clean end-of-body. No special end marker on the wire.

Run:

```bash
cargo test -p rpc-streaming-example
```

## Consuming the stream as a real client (not buffered)

The tests above prove the server side. On the **client** side,
`cratestack-client-rust` ships two consumer shapes:

- **`CratestackClient::post_list(...)`** — buffers the whole response
  body, then decodes. Same `Vec<T>` shape whether the response was
  framed as a single CBOR vec or as cbor-seq.

- **`CratestackClient::post_list_streamed(...)`** — returns a
  `tokio::sync::mpsc::Receiver<Result<T, ClientError>>` that yields
  items **as bytes arrive on the wire**. First-item latency drops from
  "buffer the whole body" to "decode one chunk." Useful on mobile /
  flaky networks where time-to-first-byte matters more than total
  throughput.

  See `crates/cratestack-client-rust/tests/streaming.rs` for the
  end-to-end demo: spins up a server that emits one chunk every 50ms,
  asserts the first item arrives before the connection has closed.

For a runnable client-side demo see [`examples/rpc-streaming-client-rust/`](../rpc-streaming-client-rust/) — a sibling crate that builds an `RpcClient` and consumes this server's `procedure.ticks` stream via `RpcClient::call_streaming::<TickerInput, Tick>("procedure.ticks", &input)`. Items arrive on a bounded `mpsc::Receiver` as cbor-seq frames parse off the wire.

For Flutter mobile clients the same capability is exposed through several entrypoints in `cratestack-client-flutter`:

- `FlutterRuntime::execute_streamed(request, on_chunk)` — REST-shaped streaming for any `application/cbor-seq` URL.
- `FlutterRuntime::rpc_call_streamed(op_id, input, headers, on_chunk)` — RPC-shaped streaming dedicated to `POST /rpc/{op_id}`.
- `FlutterCborSeqDecoder` — decode-only FFI primitive for apps that prefer to run the HTTP via `dio` (native NSURLSession / OkHttp, system proxy integration, dio interceptors). Pair it with the Dart-side `CborSeqStreamTransformer` shipped by `cratestack-client-dart` for an idiomatic `Stream<Uint8List>.transform(...)` pipeline.

## SSE?

The framework's codec layer supports `text/event-stream` too — clients
that need EventSource compatibility (browser `EventSource` API) get the
same chunk shape with SSE framing. Not exercised in this example because
the smoke tests focus on the structured-binary path that mobile and
server-to-server clients use.

## Read next

- [`rpc-procedures`](../rpc-procedures) — the smallest RPC server.
- [`rpc-batch`](../rpc-batch) — multiple ops in one POST.
- [ADR 0005: RPC binding](https://docs.cratestack.dev/internals/rpc-transport-adr).
