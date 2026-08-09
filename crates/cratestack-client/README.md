# cratestack-client

The pure HTTP-client SDK facade for CrateStack (cratestack#490): the
`include_client_schema!` macro, the generated Rust client runtime, and the
shared schema surface generated client code actually references — with
`cratestack-axum` (and therefore `axum`/`tower`/`hyper`/`tower-http`, a full
server framework) structurally absent from the dependency graph.

## When to use this crate

Pick `cratestack-client` for a crate that **only ever calls** a cratestack
server — a published SDK crate, a downstream service's typed client
dependency, a CLI that talks to a cratestack backend — and never runs one
itself. If the crate ever needs `include_server_schema!` or
`include_embedded_schema!`, it's on the wrong facade: this crate re-exports
`include_client_schema!` and nothing else, so reaching for either of the
other two macros here fails with a plain
`cannot find macro \`include_server_schema\` in \`cratestack\`` — not a
confusing wall of missing-symbol errors once the schema macro expands.

`cratestack-pg`, `cratestack-api`, and `cratestack-sqlite` all re-export
`include_client_schema!` too (a server or embedded crate can embed a client
for calling peers), and continue to work for that. This crate exists for
the case none of those three actually target: a crate that is *only* a
client, where every byte of `axum`/`tower`/`hyper` those three facades drag
in regardless is pure, permanent dead weight in the dependency graph. See
[cratestack#490](https://github.com/cratestack/cratestack/issues/490) for
the measured impact this fixes.

## Installation

Schema macros emit `::cratestack::*` paths. Alias this crate as
`cratestack` via Cargo's `package =` field:

```toml
[dependencies]
cratestack = { package = "cratestack-client", version = "0.7" }
```

Then in code:

```rust,ignore
cratestack::include_client_schema!("schema/foo.cstack");
```

`schema/foo.cstack`'s `datasource { ... }` block is irrelevant to the
client macro — the same schema a `db = Postgres` or `db = None` server
declares works unchanged here; only the `model`/`type`/`enum`/`procedure`
declarations matter.

## What this crate does not support

- **`include_server_schema!` / `include_embedded_schema!`** — not
  re-exported, deliberately. A consumer reaching for either gets a plain
  Rust name-resolution error at the call site.
- **`transport grpc`** — this crate has no `grpc` Cargo feature, so a
  `transport grpc` schema fails to compile with `cratestack-macros`'s
  existing "enable the feature" `compile_error!`
  (`crates/cratestack-macros/src/include/reject_grpc.rs`).
  `cratestack-client-rust`'s own `grpc` feature pulls in `tonic`, which
  pulls in `axum` transitively — enabling it here would defeat the entire
  point of this facade for the one consumer who opts in. A consumer that
  genuinely needs a native gRPC client should depend on
  [`cratestack-client-rust`](../cratestack-client-rust) directly with
  `features = ["grpc"]` (accepting `tonic`/`axum` into that one crate's own
  graph, deliberately), or use `cratestack-pg`/`cratestack-api` with their
  own `grpc` feature if the consumer is a server that also embeds a client.
  `cratestack generate-proto` can still emit the schema's `.proto` contract
  for use with a non-CrateStack gRPC client either way.
- **`extension rate_limit { }` / `extension pgvector { }`** — same as
  `cratestack-api`: neither Cargo feature is forwarded here, so a schema
  declaring either extension fails the same
  `extension_gate.rs` `compile_error!` under this facade as it does under
  `cratestack-api` today.

## Proving `cratestack-axum` is absent

`examples/client-only-verification` (its own standalone `[workspace]`
root, not a member of this repository's workspace — Cargo unifies a
dependency's features across every workspace member building it in the
same session, so proving absence needs to step outside that shared graph
entirely, same precedent as `examples/no-database-verification-api`,
cratestack#347) is a real, compiling client crate against this facade with
its own committed `Cargo.lock`. Reproduce the proof yourself:

```bash
cd examples/client-only-verification

# axum/tower/hyper absent under default features — no feature required
cargo tree --locked | grep -i axum   # -> no output
cargo tree --locked | grep -i tower  # -> no output
cargo tree --locked | grep -i hyper  # -> no output

# the generated client actually works
cargo test --locked
```

CI's `facade-disjointness` job (`.github/workflows/ci.yml`) re-runs this
exact proof on every PR.

## Features

- `decimal-rust-decimal` *(default)* — `Decimal`-typed procedure args/model
  fields use `rust_decimal`. Forwards to `cratestack-core` and
  `cratestack-sql` — there is no sqlx-backed half to gate, unlike
  `cratestack-pg`'s same-named feature.
- `codec-json` *(default)* — forwards the JSON codec to the generated
  client runtime, alongside CBOR.

There is no `grpc` feature — see "What this crate does not support" above.
