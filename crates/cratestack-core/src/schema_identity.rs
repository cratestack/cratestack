//! The canonical schema identity: the 32 bytes both ends of a signed
//! exchange bind into the COSE AAD as `schema_sha` (ADR 0006 §4,
//! cratestack#1065).
//!
//! It identifies the schema's *meaning*, not its source text. The macros,
//! the CLI generators and every runtime take it from [`schema_digest`], so a
//! comment, a `///` doc, an indentation change or a re-ordered declaration
//! cannot make two builds of one contract disagree — a disagreement that
//! signing turns into a 401 at deploy.
//!
//! The digest is `SHA-256(DOMAIN || canonical JSON)`. The JSON is built
//! node by node in [`nodes`] rather than by serialising [`Schema`]: a new IR
//! field must be added there on purpose, never leak into every digest
//! through a `#[serde(default)]`. What is dropped: source spans, `///` docs
//! and the whitespace of attribute text ([`attribute_norm`]). What is
//! sorted: top-level declarations and model/type/mixin/view fields, by name.
//! What keeps declared order: enum variants (the first is the `Default`,
//! and Postgres orders an enum by declaration), attributes and arguments.
//! A server-only edit (a policy, an index, a view's SQL) still changes the
//! digest: the IR is hashed whole, so a wire mismatch can never slip through.

mod attribute_norm;
mod nodes;
#[cfg(test)]
mod tests;

use sha2::{Digest, Sha256};

use crate::schema::Schema;

pub use attribute_norm::normalize_attribute_text;

/// Domain-separation tag, versioned so a change to the canonical form is a
/// deliberate new digest rather than a silent one.
pub const SCHEMA_IDENTITY_DOMAIN: &[u8] = b"cratestack/schema-identity/v1\0";

/// The canonical identity of `schema`, as raw SHA-256 bytes.
///
/// ```
/// let schema: cratestack_core::Schema = serde_json::from_str(
///     r#"{"datasource":null,"auth":null,"config_blocks":[],"mixins":[],
///         "models":[],"types":[],"enums":[],"procedures":[]}"#,
/// )
/// .unwrap();
/// assert_eq!(cratestack_core::schema_digest(&schema).len(), 32);
/// ```
pub fn schema_digest(schema: &Schema) -> [u8; 32] {
    let canonical = nodes::canonical_schema(schema);
    let json = serde_json::to_vec(&canonical).expect("a JSON value always serializes");
    let mut hasher = Sha256::new();
    hasher.update(SCHEMA_IDENTITY_DOMAIN);
    hasher.update(&json);
    hasher.finalize().into()
}

/// [`schema_digest`] as 64 lowercase hex digits (the `SCHEMA_SHA256` string).
pub fn schema_digest_hex(schema: &Schema) -> String {
    schema_digest(schema)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
