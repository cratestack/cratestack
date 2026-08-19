# client-only-verification

This crate is cratestack#490's headline piece of evidence: a real `cargo
tree` proof that `cratestack-axum` — and therefore `axum`/`tower`/`hyper`/
`tower-http`, a full server framework — is absent from a `cratestack-client`
consumer's dependency graph.

It is deliberately **not** a workspace member (see its entry in the parent
repo's root `Cargo.toml` `[workspace] exclude` list, and its own
`[workspace]` table) — same reasoning as
[`examples/no-database-verification-api`](../no-database-verification-api)
(cratestack#347): Cargo unifies a shared dependency's features across every
workspace member that builds it in the same session, so proving absence
requires stepping outside that shared graph entirely.

Unlike that crate, there's no Cargo feature to toggle here for a "present"
half of the proof: this crate only ever builds `cratestack-client` with its
default features, and `cratestack-client` has no feature that could ever
pull `axum` in transitively — `crates/cratestack-client/Cargo.toml`'s
`[features]` table only has `decimal-rust-decimal`/`decimal-bigdecimal`/
`codec-json` plus the two extension-declaration gates, none of which touch
`cratestack-axum`.
(`cratestack-client` never forwarded a `grpc` feature of its own, precisely
to avoid `cratestack-client-rust`'s former `grpc` feature pulling `tonic` —
and through it, `axum` — into this crate's graph; now that gRPC/protobuf
support has been removed entirely, `cratestack-client-rust` has no `grpc`
feature left to avoid either, see `docs/adr/0017-remove-grpc-protobuf.md`.)

## The schema

`schema.cstack` is an ordinary schema — the same shape a real
`db = Postgres` server would declare (`datasource { provider =
"postgresql" }`, a `model`, a `procedure`) — consumed here purely through
`include_client_schema!`. The `datasource` block is irrelevant to the
client macro; only the `model`/`type`/`procedure` declarations matter.
`src/lib.rs` builds a real generated client from it, and `tests/smoke.rs`
round-trips both a model list call and a procedure call over a real
(non-`axum`) HTTP server, so this isn't just a `Cargo.toml` assertion — the
generated code actually works with zero `axum` in the build.

## Run the proof yourself

```bash
cd examples/client-only-verification

# axum absent — cratestack-client never depends on cratestack-axum, no
# feature required
cargo tree --locked | grep -i axum   # -> no output

# tower/hyper/tower-http ARE present, but only via `reqwest` (the HTTP
# CLIENT transport `cratestack-client-rust` builds on) — not via
# cratestack-axum, which is what actually matters here. See "Why tower/
# hyper aren't asserted absent" below.

# the generated client works
cargo test --locked
```

## The mechanism

`crates/cratestack-client/Cargo.toml` simply has no `cratestack-axum` entry
in `[dependencies]`, under any feature — see that crate's own `lib.rs`
module doc for the full empirically-derived re-export list and why each
symbol is there.

## Why `tower`/`hyper`/`tower-http` aren't asserted absent

`cargo tree` on this crate does show `tower`, `hyper`, and `tower-http` —
but they arrive through `reqwest` (`cratestack-client-rust`'s HTTP client
transport; `reqwest` 0.12+ builds its client internals on a `tower`-based
`hyper` stack), not through `cratestack-axum`. That's an unavoidable cost
of offering an HTTP client at all, identical for every reqwest-based Rust
HTTP client on crates.io — it is not the "pull in a whole server framework"
tax this crate exists to prove absent. `axum` itself is the one dependency
that would only ever arrive via `cratestack-axum`, so it's the one this
crate's CI check greps for; asserting `tower`/`hyper` absent would be
asserting something false and unrelated to what cratestack#490 is about.
