# rpc-batch example

RPC server demonstrating `POST /rpc/batch` — multiple op calls in one
round-trip, per-frame error isolation, request order preserved on the
response envelope.

## What you'll see

- Three pure procedures (`add`, `multiply`, `divide`) so the demo focuses
  on the **batch wire shape**, not the underlying ops.
- The batch envelope: send N `RpcRequest { id, op, input, idem? }`
  frames, get N `RpcResponseFrame { id, output? | error? }` back.
- Response order matches request order — clients without correlation
  logic can zip without consulting `id`.
- One bad frame (e.g. `divide` by zero) **doesn't poison** the batch.
  Other frames succeed; the bad frame carries an `RpcErrorBody`.
- `Idempotency-Key` HTTP header is rejected on `/rpc/batch` as
  ambiguous; idempotency is always per-frame via the `idem` field.

## Run the server

```bash
cargo run -p rpc-batch-example
```

Binds on `127.0.0.1:3002`. The server logs an example curl with a
mixed batch.

## Read the smoke tests

`tests/smoke.rs`:

- `batch_preserves_request_order_on_the_response` — three ops with
  non-monotonic ids; response order matches request order.
- `per_frame_error_does_not_poison_the_batch` — `divide` by zero in
  the middle of a valid batch; envelope still 200, only the bad frame
  errors with `failed_precondition`.
- `unknown_op_in_batch_returns_per_frame_not_found` — unknown op as
  one frame; per-frame `not_found` error.
- `idempotency_key_header_is_rejected_on_batch` — `Idempotency-Key`
  HTTP header → 400.

Run:

```bash
cargo test -p rpc-batch-example
```

## Strict batch — what isn't supported

- **No transactional mode.** Each frame runs in its own transaction.
  Mixing reads and writes inside a batch is fine; rolling them back
  together is not a v1 feature.
- **No in-batch dependencies.** A batch like
  `[create A, update B referencing A.id]` is not supported. The correct
  shapes are (a) two roundtrips, or (b) a single `@procedure` that owns
  the composite operation.
- **No client-driven parallelization.** Server processes frames
  sequentially in v1. The design permits parallelization once contention
  is observable.

See [ADR 0005 §3.2](https://docs.cratestack.dev/internals/rpc-transport-adr)
for the design rationale.

## Read next

- [`rpc-batch-debounce`](../rpc-batch-debounce) — client-side debouncer
  that coalesces single calls into a single batch round-trip.
- [`rpc-procedures`](../rpc-procedures) — the smallest RPC server.
- [`rpc-streaming`](../rpc-streaming) — list-return procedure via
  `Accept: application/cbor-seq`.
