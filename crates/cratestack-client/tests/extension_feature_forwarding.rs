//! Proof that this facade's `pgvector`/`rate_limit` Cargo features really
//! forward down to `cratestack-macros`' declaration gate
//! (`include/extension_gate.rs`'s `guard_client_declared_extensions`), so a
//! client SDK can be generated against a schema that declares either
//! extension.
//!
//! Found in review of cratestack#490: this facade originally shipped with
//! neither feature declared at all, while the gate `include_client_schema!`
//! runs is the same one the server/embedded macros run. That combination
//! made any schema containing `extension pgvector { }` or `extension
//! rate_limit { }` a hard `compile_error!` through this facade with no
//! feature to opt into — reachable only by reaching around it and adding a
//! direct `cratestack-macros` dependency, which the facade's own docs treat
//! as a private implementation detail. Since schemas are shared between the
//! server and client roles, that ruled out generating a client for any
//! server schema using embeddings or rate limiting.
//!
//! Gated `required-features = ["pgvector", "rate_limit"]` in `Cargo.toml`,
//! mirroring `cratestack-pg`'s `pgvector_feature_forwarding` test. To see
//! this fail the way it did before the fix, drop either feature from the
//! `[[test]]` entry and from the `cargo test` invocation — the
//! `include_client_schema!` below then stops compiling with the gate's own
//! `compile_error!`. The generic negative case for the gate itself is
//! already covered by `cratestack-macros`' `trybuild` suite (#161).

mod extensions_schema {
    cratestack::include_client_schema!("tests/fixtures/extensions.cstack");
}

/// A `Vector(n)` field reaches the generated *client* struct as a plain
/// `Vec<f32>` — the `pgvector` crate is never referenced on this path (it
/// enters only at the sqlx row-decode/bind boundary, which is server-only
/// and structurally absent from this facade). This is what makes
/// forwarding to `cratestack-macros` alone, with no runtime half, the
/// correct wiring here rather than an omission.
#[test]
fn vector_field_reaches_the_client_struct_as_vec_f32() {
    let document = extensions_schema::cratestack_schema::Document {
        id: 1,
        title: "embedding carrier".to_owned(),
        embedding: vec![0.1_f32, 0.2, 0.3],
    };

    assert_eq!(document.embedding, vec![0.1_f32, 0.2, 0.3]);
    assert_eq!(document.title, "embedding carrier");
}

/// The `@no_rate_limit` attribute parses and the procedure it annotates
/// still generates its normal client surface. Rate limiting itself is
/// enforced in `cratestack-axum` and has no client-side half — the feature
/// exists here purely so the schema is accepted.
#[test]
fn no_rate_limit_procedure_still_generates_its_client_args() {
    let args = extensions_schema::cratestack_schema::ReindexArgs { documentId: 7 };
    let result = extensions_schema::cratestack_schema::ReindexResult { queued: true };

    assert_eq!(args.documentId, 7);
    assert!(result.queued);
}
