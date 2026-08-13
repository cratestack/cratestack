//! `extension rate_limit { }` + `@no_rate_limit` end-to-end smoke test
//! (cratestack#154). Gated on the `rate_limit` Cargo feature — without it,
//! `include_server_schema!` against a schema declaring `extension
//! rate_limit { }` is a `compile_error!` by design (see
//! `crates/cratestack-macros/src/include/extension_gate.rs`), so this whole
//! file is skipped rather than failing a default `cargo test -p
//! cratestack-pg` run. Run with `cargo test -p cratestack-pg --features
//! rate_limit --test rate_limit_extension`.
//!
//! This ticket is purely about making rate-limit *participation*
//! schema-visible on the generated `OpDescriptor` — it does not construct
//! or exercise `cratestack_axum::ratelimit::RateLimitLayer` at all (that
//! machinery is untouched, still assembled entirely imperatively by the
//! consuming app; see the ticket for why). So there is nothing here to spin
//! up a live rate limiter for — the assertions are all against the
//! generated `OPS` const.

#![cfg(feature = "rate_limit")]

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;

include_server_schema!("tests/fixtures/rate_limit_extension.cstack", db = Postgres);

#[allow(dead_code)]
fn test_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

/// AC 1: a procedure marked `@no_rate_limit` in a schema declaring
/// `extension rate_limit { }` gets `rate_limited_by_default: false` on its
/// generated `OpDescriptor`.
#[test]
fn no_rate_limit_procedure_op_is_not_rate_limited_by_default() {
    let ops = cratestack_schema::axum::OPS;

    let create_payment = ops
        .iter()
        .find(|op| op.op_id == "procedure.createPayment")
        .expect("procedure.createPayment should be emitted");

    assert!(
        !create_payment.rate_limited_by_default,
        "@no_rate_limit procedure should have rate_limited_by_default: false",
    );
}

/// AC 1 (converse): every op that does *not* carry `@no_rate_limit` —
/// procedures and model CRUD verbs alike — defaults to
/// `rate_limited_by_default: true`.
#[test]
fn every_other_op_defaults_to_rate_limited_by_default_true() {
    let ops = cratestack_schema::axum::OPS;
    assert!(!ops.is_empty(), "fixture should emit at least one op");

    for op in ops {
        if op.op_id == "procedure.createPayment" {
            continue;
        }
        assert!(
            op.rate_limited_by_default,
            "{} should default to rate_limited_by_default: true",
            op.op_id,
        );
    }

    // Sanity: the schema actually has model CRUD ops in the mix, not just
    // the two procedures — otherwise the loop above would trivially pass.
    assert!(
        ops.iter().any(|op| op.op_id == "model.Widget.list"),
        "fixture should emit model CRUD ops too",
    );
}
