# rpc-batch-debounce example

Client-side **batch debouncer** that coalesces independent RPC calls into
a single `POST /rpc/batch`. This is the shape most offline-first UIs and
rate-sensitive callers want: each `.call(op, input).await` looks like a
normal request to the caller, but under the hood many of them share one
TCP round-trip.

## What you'll see

- A `BatchDebouncer` that owns a service (in the example: an in-process
  `axum::Router` from the rpc-batch example, but in production this would
  wrap a real HTTP client).
- `.call(op, input).await` returns a future. Internally it pushes a frame
  + a `oneshot::Sender` onto the pending buffer and parks the caller.
- Two auto-flush triggers:
  - **Size**: pending buffer reaches `max_size`.
  - **Manual**: caller invokes `debouncer.flush().await`.
- On flush: build one batch body, send it, decode the response, fan each
  `RpcResponseFrame` back to its waiting caller by correlation `id`.

(No time-based auto-flush is built in — it keeps the type deterministic
for tests. If you want one, spawn a tokio task that calls `flush()` on
an interval. See `main.rs`.)

## Run the demo

```bash
cargo run -p rpc-batch-debounce-example
```

Output looks like:

```
Issuing 12 .call() invocations with a 4-call debouncer:
  call  0 (procedure.add        ) -> {"value":0}
  call  1 (procedure.multiply   ) -> {"value":2}
  ...
Done in 1.4ms. Three batches landed (12 calls / 4-call window).
```

## Read the smoke tests

`tests/smoke.rs`:

- `fewer_than_max_size_calls_wait_for_explicit_flush` — three calls
  enqueue and park; nobody resolves until `flush()` runs.
- `hitting_max_size_triggers_auto_flush` — the size limit auto-flushes
  without any explicit `.flush()` call.
- `per_call_errors_route_to_the_right_awaiter` — mixed batch with one
  error frame; the error lands on the right caller's await without
  contaminating the others.
- `empty_flush_is_a_noop` — `flush()` on an empty buffer is harmless.

Run:

```bash
cargo test -p rpc-batch-debounce-example
```

## Adding a time-based auto-flush

The `BatchDebouncer` keeps itself deterministic — no internal timers.
For a UI that wants "flush every 100ms or when 32 calls accumulate,
whichever comes first," spawn this alongside the debouncer:

```rust
let d = debouncer.clone();
tokio::spawn(async move {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        tick.tick().await;
        let _ = d.flush().await;
    }
});
```

That keeps the timing policy at the caller, where different surfaces
(mobile vs server) often want different windows.

## Why client-side debouncing is more useful than server-side

The batch route is **server-side**: it accepts N frames and runs them.
The savings come from the **client** choosing to batch in the first
place. Without a debouncer, twelve UI handlers that each call
`.call()` make twelve HTTP round-trips even though the server-side
batch route is sitting right there. The debouncer makes "I want to call
this op" the API and "use one HTTP round-trip" the implementation.

## Read next

- [`rpc-batch`](../rpc-batch) — the server side this example talks to.
- [`rpc-procedures`](../rpc-procedures) — the smallest RPC server, no batch.
- [ADR 0005: RPC binding](https://docs.cratestack.dev/internals/rpc-transport-adr).
