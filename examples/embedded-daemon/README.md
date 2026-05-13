# embedded-daemon

Long-running Rust daemon that watches a directory with [`notify`](https://docs.rs/notify), debounces bursty filesystem events, and persists one row per `(path, settle-window)` pair through `include_embedded_schema!` + `cratestack-rusqlite`.

This is the canonical **"tokio async I/O on the outside, sync `ModelDelegate` on the inside"** example. The `RusqliteRuntime` is `Send + Sync` but its API is blocking, so every persistence call goes through `tokio::task::spawn_blocking` — the seam most users hit the first time they pair CrateStack with `axum` / `tokio` / any async server runtime.

## Layout

```
embedded-daemon/
├── Cargo.toml
├── schema.cstack             # FileEvent model
├── src/
│   ├── lib.rs                # include_embedded_schema! + Debouncer + persist_event
│   └── main.rs               # tokio main + notify wiring + spawn_blocking flush
└── README.md
```

The split is deliberate: everything that can be unit-tested without a tokio runtime or a real filesystem lives in `lib.rs` (the debouncer is pure state, `persist_event` runs against an in-memory SQLite). `main.rs` is the thinnest possible wrapper that bolts on `notify` + `tokio::select!`.

## Schema

```cstack
model FileEvent {
  id Uuid @id
  path String
  kind String         // created | modified | renamed | deleted
  observedAt DateTime
  bursts Int          // how many raw notify events collapsed into this row
}
```

`bursts` is the artifact of the debouncer — if you save a file 30 times in 250 ms, you get **one** `FileEvent` row with `bursts = 30`, not thirty rows.

## How it runs

```bash
cargo run -p embedded-daemon-example -- \
  --watch /tmp/watch-me \
  --db /tmp/events.db \
  --window-ms 250 \
  --tick-ms 100
```

Then in another shell:

```bash
mkdir -p /tmp/watch-me
echo hello > /tmp/watch-me/a.txt        # one row, kind=created
for i in 1 2 3 4 5; do echo $i >> /tmp/watch-me/a.txt; done   # one row, kind=modified, bursts≈5
rm /tmp/watch-me/a.txt                  # one row, kind=deleted
```

Then read back:

```bash
sqlite3 /tmp/events.db 'select path, kind, bursts, observed_at from FileEvent order by observed_at;'
```

`Ctrl-C` flushes any pending debounced entry before exiting — no rows are lost on shutdown.

## The async/sync seam

The interesting bits in `main.rs`:

```rust
let runtime = Arc::new(RusqliteRuntime::open(&cli.db)?);
// ...
async fn flush_batch(runtime: &Arc<RusqliteRuntime>, batch: Vec<ReadyEvent>) {
    let runtime = Arc::clone(runtime);
    tokio::task::spawn_blocking(move || {
        for event in batch {
            persist_event(&runtime, event)?;
        }
        Ok::<_, RusqliteError>(())
    }).await...
}
```

`RusqliteRuntime` is not `Clone` (the underlying `Mutex<Connection>` shouldn't be silently duplicated — see the doc comment on the type), so it's wrapped in `Arc` once at startup. Each batch flush clones the `Arc` and moves it into the blocking task, which holds the mutex for the duration of the inserts. The tokio worker stays free to handle the next `select!` arm.

The `notify` callback runs on `notify`'s own internal thread, **not** a tokio task. It pushes raw events into a `tokio::sync::mpsc::unbounded_channel` (whose `send` is non-async and safe from any thread). That's the bridge from the OS-fd-driven world into the tokio reactor.

## Tests

```bash
cargo test -p embedded-daemon-example
```

Four tests, all in `lib.rs`:

- `burst_of_modifies_collapses_to_one_row` — debouncer math
- `delete_after_modify_wins` — kind precedence (`deleted` always wins)
- `different_paths_drain_independently` — per-path windows don't interfere
- `persist_round_trip_against_in_memory_db` — `bootstrap` + `persist_event` exercised end-to-end against `RusqliteRuntime::open_in_memory()`

## See also

- [`embedded-cli`](../embedded-cli) — same `include_embedded_schema!` shape, synchronous `clap` CLI (no tokio, no async).
- [`embedded-webhook`](../embedded-webhook) — the other "async-around-sync" example: axum + `include_embedded_schema!`, server-shaped instead of daemon-shaped.
- [Phase A — Pure Rust](../README.md#phase-a--pure-rust-shipped-in-this-release) — the rest of the cargo-native examples.
