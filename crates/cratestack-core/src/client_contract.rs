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
//! on the reviewed [`DROPPED_ATTRIBUTES`] list are filtered; every other
//! attribute, known or not, stays in, so a new one moves digests until it is
//! reviewed onto that list. The digest is
//! `SHA-256(OP_CONTRACT_DOMAIN || canonical JSON)`; if the derivation rules
//! change, the domain tag moves to `op-contract/v2`.
//!
//! Nothing binds these yet: the COSE AAD still carries the whole-IR
//! identity (binding v1). `cratestack contract digest|print` shows them.

mod attrs;
mod canon;
mod ops;
mod project;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bytes;
#[cfg(test)]
mod tests_drop;

use sha2::{Digest, Sha256};

use crate::events::{ModelEventKind, parse_emit_attribute};
use crate::schema::{Procedure, ProcedureKind, Schema, TransportStyle, TypeArity};
use crate::schema_identity::members::args;
use crate::schema_identity::members::type_ref;

pub use attrs::{DROPPED_ATTRIBUTES, attribute_name, is_dropped};
pub use ops::{ClientOp, ModelVerb, OpTarget, ops};

use canon::{COpContract, CWireProcedure};

/// Domain-separation tag of an op digest.
pub const OP_CONTRACT_DOMAIN: &[u8] = b"cratestack/op-contract/v1\0";
/// Domain-separation tag of [`client_contract_digest`].
pub const CLIENT_CONTRACT_DOMAIN: &[u8] = b"cratestack/client-contract/v1\0";

fn contract<'a>(schema: &'a Schema, op: &'a ClientOp<'a>) -> COpContract<'a> {
    let transport = match schema.transport {
        TransportStyle::Rpc => "rpc",
        TransportStyle::Rest => "rest",
    };
    match op.target {
        OpTarget::Model(model, verb) => COpContract {
            closure: project::closure(schema, &[&model.name]),
            events: (verb == ModelVerb::Subscribe).then(|| emitted(model)),
            key: &op.key,
            kind: if verb == ModelVerb::Subscribe { "subscription" } else { "unary" },
            model: Some(&model.name),
            procedure: None,
            transport,
            verb: verb.as_str(),
        },
        OpTarget::Procedure(p) => procedure_contract(schema, op, p, transport),
    }
}

fn procedure_contract<'a>(
    schema: &'a Schema,
    op: &'a ClientOp<'a>,
    p: &'a Procedure,
    transport: &'static str,
) -> COpContract<'a> {
    let Procedure {
        docs: _,
        name,
        name_span: _,
        kind,
        args: procedure_args,
        return_type,
        attributes,
        span: _,
        mcp: _,
    } = p;
    let mut roots: Vec<&str> = vec![&return_type.name];
    roots.extend(return_type.generic_args.iter().map(|g| g.name.as_str()));
    for arg in procedure_args {
        roots.push(&arg.ty.name);
        roots.extend(arg.ty.generic_args.iter().map(|g| g.name.as_str()));
    }
    let kind = match kind {
        ProcedureKind::Query => "query",
        ProcedureKind::Mutation => "mutation",
    };
    COpContract {
        closure: project::closure(schema, &roots),
        events: None,
        key: &op.key,
        kind: if return_type.arity == TypeArity::List { "sequence" } else { "unary" },
        model: None,
        procedure: Some(CWireProcedure {
            args: args(procedure_args),
            attributes: project::wire_attributes(attributes),
            kind,
            name,
            return_type: type_ref(return_type),
        }),
        transport,
        verb: kind,
    }
}

fn emitted(model: &crate::schema::Model) -> Vec<&'static str> {
    let mut kinds: Vec<ModelEventKind> = model
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@@emit("))
        .filter_map(|a| parse_emit_attribute(&a.raw).ok())
        .flatten()
        .collect();
    kinds.sort_by_key(|k| k.as_str());
    kinds.dedup();
    kinds.into_iter().map(ModelEventKind::as_str).collect()
}

fn canonical(schema: &Schema, op: &ClientOp<'_>) -> Vec<u8> {
    serde_json::to_vec(&contract(schema, op)).expect("plain structs always serialize")
}

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
