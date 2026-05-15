# rpc-procedures example

Smallest possible CrateStack RPC server: one query procedure (`greet`) and
one mutation procedure (`increment`). No database, no models — the example
focuses on the **RPC binding's unary route shape**.

## What you'll see

- A `.cstack` schema declaring `transport rpc` and two procedures.
- The macro emits an `rpc_router(...)` builder that mounts
  `POST /rpc/{op_id}`.
- The op id appears in the URL: `POST /rpc/procedure.greet`,
  `POST /rpc/procedure.increment`.
- The body is the procedure's `Args` struct *directly* — no envelope.
- Errors come back as `RpcErrorBody { code, message, details? }` with
  gRPC-style lowercase codes (`permission_denied`, `not_found`, …).

## Run the server

```bash
cargo run -p rpc-procedures-example
```

The server logs the curl invocation you can use to hit it. In another
terminal:

```bash
# query — anonymous fails with permission_denied (status 403)
curl -X POST http://127.0.0.1:3000/rpc/procedure.greet \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -d '{"args":{"name":"world"}}'

# query — authenticated succeeds
curl -X POST http://127.0.0.1:3000/rpc/procedure.greet \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -H 'x-auth-id: 1' \
  -d '{"args":{"name":"world"}}'

# mutation — stateful counter
curl -X POST http://127.0.0.1:3000/rpc/procedure.increment \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -H 'x-auth-id: 1' \
  -d '{"args":{"by":5}}'
```

## Read the smoke tests

`tests/smoke.rs` is the actual documentation. Each test demonstrates one
wire-shape contract:

- `greet_procedure_round_trips_over_json` — happy path over JSON.
- `greet_procedure_round_trips_over_cbor_by_default` — content
  negotiation; CBOR is the binding's default response codec.
- `increment_mutation_is_stateful_across_calls` — the registry's
  in-memory state survives across requests.
- `unauthenticated_call_is_denied_with_lowercase_grpc_code` — proves the
  error shape is `RpcErrorBody` with `code: "permission_denied"`, not
  the REST binding's `FORBIDDEN`.

Run them via:

```bash
cargo test -p rpc-procedures-example
```

No database setup required — the lazy pg pool is never opened because the
procedures don't touch it.

## Read next

- [`rpc-streaming`](../rpc-streaming) — list-return procedure streamed via
  `Accept: application/cbor-seq`.
- [`rpc-batch`](../rpc-batch) — multiple ops in one `POST /rpc/batch`.
- [`rpc-batch-debounce`](../rpc-batch-debounce) — client-side debouncer
  that coalesces calls into a single batch.
- [ADR 0005: RPC binding](https://docs.cratestack.dev/internals/rpc-transport-adr)
  for the design.
