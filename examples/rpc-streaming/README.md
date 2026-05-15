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
