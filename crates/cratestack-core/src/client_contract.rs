//! Per-op contract digests (cratestack#1123, EXT-14): the 32 bytes that move
//! only when the wire shape of *one op* changes.
//!
//! [`schema_digest`](crate::schema_digest) hashes the whole IR, so a policy,
//! an `@@index`, a view's SQL or a new procedure changes it and every signed
//! client built before that edit is refused. An op's contract is the closure
//! a peer decodes it under: its transport, key and kind, its input and
//! output roots, and every model, type, enum and view reachable from them
//! (through fields, generic arguments, relations and `@computed(params:)`),
//! each in its wire projection (`@server_only` fields removed).
//!
//! Out entirely: the datasource, config blocks, `extension` blocks, the
//! `auth` block, the schema-level MCP config, mixins (the parser expands
//! them into fields), queries, every other op, docs and spans. Attributes
//! on the reviewed `DROPPED_ATTRIBUTES` list are filtered; every other
//! attribute, known or not, stays in, so a new one moves digests until it is
//! reviewed onto that list. The digest is
//! `SHA-256(OP_CONTRACT_DOMAIN || canonical JSON)`; if the derivation rules
//! change, the domain tag moves to `op-contract/v2`.
//!
//! The COSE AAD binds the digest of the op a message calls (binding
//! version 2; ADR 0006 §4); [`table`] holds the tables generated code
//! carries and how a route finds its row. `cratestack contract
//! digest|print` shows the digests.

mod attrs;
mod build;
mod canon;
mod ops;
mod project;
mod table;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bytes;
#[cfg(test)]
mod tests_drop;
#[cfg(test)]
mod tests_ops;
#[cfg(test)]
mod tests_readers;
#[cfg(test)]
mod tests_readers_symbols;
#[cfg(test)]
mod tests_readers_table;
#[cfg(test)]
mod tests_table;

use sha2::{Digest, Sha256};

use crate::schema::Schema;
use build::canonical;
use ops::ops;

pub use ops::op_keys;
pub use table::{
    AcceptedContracts, BATCH_CONTRACT_KEY, OpContracts, bound_contracts, find_contract,
};

/// Domain-separation tag of an op digest.
pub const OP_CONTRACT_DOMAIN: &[u8] = b"cratestack/op-contract/v1\0";
/// Domain-separation tag of [`client_contract_digest`].
pub const CLIENT_CONTRACT_DOMAIN: &[u8] = b"cratestack/client-contract/v1\0";

fn digest_of(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(OP_CONTRACT_DOMAIN);
    hasher.update(bytes);
    hasher.finalize().into()
}

/// The canonical JSON an op digest hashes (what `cratestack contract print`
/// shows), or `None` when the schema exposes no op with that key.
pub fn op_contract_json(schema: &Schema, key: &str) -> Option<String> {
    let all = ops(schema);
    let op = all.iter().find(|op| op.key == key)?;
    Some(String::from_utf8(canonical(schema, op)).expect("JSON is UTF-8"))
}

/// The digest of the op `key` (an RPC `op_id`, or `"<METHOD> <route>"` on
/// REST), or `None` when the schema exposes no such op.
///
/// ```
/// let schema: cratestack_core::Schema = serde_json::from_str(
///     r#"{"datasource":null,"auth":null,"config_blocks":[],"mixins":[],
///         "models":[],"types":[],"enums":[],"procedures":[]}"#,
/// )
/// .unwrap();
/// assert!(cratestack_core::op_contract_digest(&schema, "procedure.ping").is_none());
/// assert!(cratestack_core::op_contract_digests(&schema).is_empty());
/// ```
pub fn op_contract_digest(schema: &Schema, key: &str) -> Option<[u8; 32]> {
    let all = ops(schema);
    let op = all.iter().find(|op| op.key == key)?;
    Some(digest_of(&canonical(schema, op)))
}

/// Every op's digest, sorted by key.
pub fn op_contract_digests(schema: &Schema) -> Vec<(String, [u8; 32])> {
    ops(schema)
        .iter()
        .map(|op| (op.key.clone(), digest_of(&canonical(schema, op))))
        .collect()
}

/// Lowercase hex of a digest.
pub fn digest_hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A build identity for the client-facing contract: a hash over the sorted
/// `[key, hex(op digest)]` table. It moves when any op's contract moves, a
/// new op appears or one disappears.
pub fn client_contract_digest(schema: &Schema) -> [u8; 32] {
    let table: Vec<[String; 2]> = op_contract_digests(schema)
        .into_iter()
        .map(|(key, digest)| [key, digest_hex(&digest)])
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(CLIENT_CONTRACT_DOMAIN);
    hasher.update(serde_json::to_vec(&table).expect("strings always serialize"));
    hasher.finalize().into()
}
