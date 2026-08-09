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
client, where the `axum` server stack those three facades drag in
regardless is pure, permanent dead weight in the dependency graph. (The
`tower`/`hyper` layer underneath it stays either way — `reqwest` needs it;
what goes is `axum` and `cratestack-axum` on top.) See
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

# axum absent under default features — no feature required
cargo tree --locked | grep -i axum   # -> no output

# the generated client actually works
cargo test --locked
```

`tower`, `hyper`, and `tower-http` **do** still appear in that tree, and
that is expected: they arrive through `reqwest`, the HTTP client
`cratestack-client-rust` is built on, not through `cratestack-axum`. No
HTTP client crate can avoid them. What this facade removes is the *server*
framework — `axum` itself, and the routing/extraction/handler machinery
`cratestack-axum` layers on top. See `examples/client-only-verification`'s
README, "Why `tower`/`hyper`/`tower-http` aren't asserted absent".

CI's `facade-disjointness` job (`.github/workflows/ci.yml`) re-runs the
`axum`-absence grep and `cargo test --locked` on every PR.

## Features

- `decimal-rust-decimal` *(default)* — `Decimal`-typed procedure args/model
  fields use `rust_decimal`. Forwards to `cratestack-core`/`cratestack-sql`/
  `cratestack-client-rust`/`cratestack-macros` — there is no sqlx-backed
  half to gate, unlike `cratestack-pg`'s same-named feature.
- `decimal-bigdecimal` — arbitrary-precision `bigdecimal` backend instead
  (heap-allocated, not `Copy` — see `cratestack-core`'s README for the
  trait differences). Mutually exclusive with `decimal-rust-decimal`;
  selecting neither or both is a compile error. `cargo tree -p
  cratestack-client --no-default-features --features decimal-bigdecimal -e
  features | grep rust_decimal` prints nothing, confirming the swap is
  complete through this facade (cratestack#495).
- `codec-json` *(default)* — forwards the JSON codec to the generated
  client runtime, alongside CBOR.
- `pgvector` — enable when the schema declares `extension pgvector { }`.
- `rate_limit` — enable when the schema declares `extension rate_limit { }`.

The last two are **schema-compatibility switches, not feature
implementations**. A client is generated from the same `.cstack` the server
is built from, so a schema declaring either extension must still compile
here — but neither extension has a client-side half. A `Vector(n)` field
arrives as a plain `Vec<f32>` (the `pgvector` crate is only involved at the
server's sqlx row-decode boundary), and `@no_rate_limit` only affects
enforcement that lives in `cratestack-axum`, which this facade has no
dependency on. So unlike `cratestack-pg`, these forward to
`cratestack-macros` alone — they exist purely to satisfy the declaration
gate that would otherwise reject the schema:

```toml
cratestack = { package = "cratestack-client", version = "0.7", features = ["pgvector"] }
```

There is no `grpc` feature — see "What this crate does not support" above.
