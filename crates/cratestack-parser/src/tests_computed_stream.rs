#![cfg(test)]

//! `@stream` never carries a model with a `@computed` field. The stream
//! encoder serializes each item through serde, so it would honour
//! `#[serde(skip)]` on a `@server_only` field but never resolve the
//! computed one; the only path that resolves it for a procedure output is
//! the compose helper, which bypasses serde and must leave `@server_only`
//! out by hand (`cratestack-macros/src/computed/compose.rs`). This pins
//! that a streamed output cannot be a third path. `tests_computed` covers
//! the same rule for a `type`; this is the `model` case, with the
//! `@server_only` field present.
//!
//! Split out of `tests_computed` rather than appended to it: that file is
//! already grandfathered past the 200-line ceiling
//! (`.ci/file-length-allowlist.toml`).

use super::parse_schema;

/// `@stream` accepts only `T[]`; both the model itself and a `type` that
/// embeds it are refused.
#[test]
fn rejects_a_stream_of_a_model_with_computed_and_server_only_fields() {
    for return_type in ["Widget[]", "Envelope[]"] {
        let source = format!(
            "model Widget {{\n  id Int @id\n  secret String @server_only\n  \
             hint String @computed\n}}\n\n\
             type Envelope {{\n  one Widget\n}}\n\n\
             procedure widgets(): {return_type}\n  @stream\n"
        );
        let error = parse_schema(&source)
            .expect_err("@stream over a computed-bearing model should fail validation");
        assert!(
            error.to_string().contains("stream encoder"),
            "{return_type}: {error}"
        );
    }
}
