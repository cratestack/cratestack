//! Compact wire keys (`docs/design/compact-wire-keys.md`, proposal C): the
//! type trees that let a codec swap a body's field names for integers.
//!
//! A key table is a pure function of an op contract
//! ([`op_contract_json`](crate::op_contract_json)): for every model, type and
//! view in the op's closure, key *n* is the *n*-th entry of that type's
//! `fields` array (already sorted by name, `@server_only` removed), and the
//! `Args` wrapper of a procedure is numbered by `procedure.args`, in
//! declaration order. Numbers need not be stable across versions, because
//! every version's tree is addressed by its own op contract digest, which the
//! client names in the `Cratestack-Contract` header ([`ContractSelector`]).
//!
//! This module holds the data model ([`KeyTables`], [`KeyNode`]), the
//! derivation from a contract, and the artifact generated clients and
//! servers embed ([`KeyTables::to_bytes`]). The byte rewriting itself lives
//! in `cratestack-codec-cbor` and `cratestack-codec-json`.
//!
//! [`ContractSelector`]: crate::ContractSelector

mod artifact;
mod derive;
mod derive_ops;
mod error;
mod media;
mod schema;
mod tables;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_artifact;
#[cfg(test)]
mod tests_lock;
#[cfg(test)]
mod tests_support;

pub use error::CompactError;
pub use media::{
    COMPACT_CBOR, COMPACT_CBOR_SEQ, COMPACT_JSON, compact_media_type_for, plain_media_type_for,
};
pub use tables::{Direction, KeyNode, KeyStruct, KeyTables, OpKeys};
