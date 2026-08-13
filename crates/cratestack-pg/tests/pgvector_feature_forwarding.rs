//! End-to-end check that this facade's `pgvector` Cargo feature really
//! forwards all the way down: `cratestack-macros/pgvector` (#161's
//! compile-time declaration gate — a schema declaring `extension
//! pgvector { }` is a `compile_error!` without it) and
//! `cratestack-sqlx/pgvector` (the real Postgres `vector` column
//! codec via the `pgvector` crate). Mirrors how #161 itself verified
//! the base feature-forwarding mechanism with a throwaway scratch
//! crate (see `docs/design/extensions.md` §2).
//!
//! Gated `required-features = ["pgvector"]` in `Cargo.toml`, so this
//! file only compiles as part of `cargo test -p cratestack-pg
//! --features pgvector`. The complementary negative case — the same
//! *declaration gate* (layer 2 of `docs/design/extensions.md` §2)
//! failing to compile with the feature disabled — is already covered
//! generically by `cratestack-macros`' own `trybuild`-based
//! `tests/ui.rs` (#161), which exercises `extension_gate.rs` directly
//! against `include_server_schema!`/`include_embedded_schema!` rather
//! than a real facade; this crate doesn't duplicate a second
//! `trybuild` setup at the facade level for the same gate.

use cratestack::include_server_schema;

include_server_schema!(
    "tests/fixtures/pgvector_feature_forwarding.cstack",
    db = Postgres
);

#[test]
fn vector_field_compiles_as_vec_f32_when_pgvector_feature_is_enabled() {
    let document = cratestack_schema::Document {
        id: 1,
        embedding: vec![0.1_f32, 0.2, 0.3],
    };

    assert_eq!(document.id, 1);
    assert_eq!(document.embedding, vec![0.1_f32, 0.2, 0.3]);
}
