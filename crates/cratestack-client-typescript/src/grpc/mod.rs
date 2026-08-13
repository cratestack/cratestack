//! gRPC-Web template-context building — ticket #172. Model CRUD only,
//! matching what `cratestack-grpc`'s macro-generated tonic service
//! actually exposes (ticket #171): procedures and server-streaming are
//! not wired into the generated gRPC service at all, so there is nothing
//! for this generator to bind a TypeScript method to for either — see
//! `crates/cratestack-grpc/src/lib.rs`'s module doc for the authoritative
//! scope statement this generator mirrors.
//!
//! Sub-concerns, one per file to stay under the repo's 200-LoC
//! convention:
//! - [`wire`] — field-level wire-kind mapping (`.cstack` scalar -> how the
//!   TS runtime encodes/decodes it).
//! - [`messages`] — the generic recursive message-descriptor collector.
//! - [`crud_messages`] — seeds that collector with every CRUD-only
//!   message shape; numbers sourced from the schema's `.pb.lock`.
//! - [`synth_fields`] — field lists for the messages this generator
//!   synthesizes rather than reads off a schema `Model`/`TypeDecl`.
//! - [`methods`] — per-model method path/name derivation, reusing
//!   `cratestack_proto::op_id_to_method_name` so a generated client's
//!   method names never drift from the Rust server's own.

mod crud_messages;
mod messages;
mod methods;
mod synth_fields;
mod wire;

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, Schema, TypeArity};
use cratestack_proto::PbLock;

pub(crate) use messages::GrpcMessageView;
pub(crate) use methods::GrpcModelView;
use serde::Serialize;

use crate::error::TypeScriptGeneratorError;
use crate::types::{enum_name_set, primary_key_field};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GrpcContext {
    pub(crate) package: String,
    pub(crate) messages: Vec<GrpcMessageView>,
    pub(crate) models: Vec<GrpcModelView>,
    pub(crate) enums: Vec<GrpcEnumWire>,
}

/// A `.cstack` enum's wire name<->number table, straight from the
/// schema's `.pb.lock` (`docs/design/protobuf.md` §4.5 — including the
/// synthetic `<NAME>_UNSPECIFIED = 0` variant, which the lock already
/// carries). Every field the runtime's `encodeMessage`/`decodeMessage`
/// classify as `"enum"` looks itself up in this table at generation time
/// via the field's `refName`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GrpcEnumWire {
    pub(crate) name: String,
    pub(crate) variants: Vec<(String, i32)>,
}

/// Builds the gRPC-Web-specific slice of the template context. Returns
/// `None` for non-`Grpc` transports (callers skip attaching it); errors
/// when a `Grpc`-transport schema has no `.pb.lock` (`MissingPbLock`,
/// `generate-proto` hasn't been run yet) or a lock entry the schema
/// expects is missing (`MissingPbLockEntry`, the lock is stale).
pub(crate) fn build_grpc_context(
    schema: &Schema,
    pb_lock: Option<&PbLock>,
) -> Result<Option<GrpcContext>, TypeScriptGeneratorError> {
    if schema.transport != cratestack_core::TransportStyle::Grpc {
        return Ok(None);
    }
    let pb_lock = pb_lock.ok_or(TypeScriptGeneratorError::MissingPbLock)?;
    let package = pb_lock
        .package
        .clone()
        .ok_or(TypeScriptGeneratorError::MissingPbLockPackage)?;

    let models_with_pk: Vec<&Model> = schema
        .models
        .iter()
        .filter(|model| primary_key_field(model).is_some())
        .collect();
    let enum_names: BTreeSet<&str> = enum_name_set(&schema.enums);

    let messages =
        crud_messages::build_grpc_messages(schema, pb_lock, &models_with_pk, &enum_names)?;
    let models = models_with_pk
        .iter()
        .map(|model| methods::build_grpc_model_view(&package, model))
        .collect();
    let enums = schema
        .enums
        .iter()
        .filter_map(|enum_decl| {
            pb_lock.enums.get(&enum_decl.name).map(|lock| GrpcEnumWire {
                name: enum_decl.name.clone(),
                variants: lock.variants.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            })
        })
        .collect();

    Ok(Some(GrpcContext {
        package,
        messages,
        models,
        enums,
    }))
}

/// `<Model>RpcListInput`'s reduced field set this generator actually
/// wires up (`limit`/`offset`/`fields`/`include`/`sort`) — the common
/// list-projection controls. Deliberately excludes `where_expr`/`or`
/// (raw predicate query strings) and `filters`
/// (`repeated RpcListPredicate`, an advanced structured-predicate
/// builder) and `include_fields` (a `map<string, StringList>`, which
/// needs protobuf map-entry encoding this pass doesn't implement) — see
/// this ticket's final report for the full reasoning. Every field left
/// out here simply never gets set on the wire, which decodes on the
/// server as "not provided" (proto3 explicit-presence `None`), the same
/// as a REST/RPC caller who never passed those query parameters.
pub(crate) fn list_input_wire_fields() -> Vec<Field> {
    use synth_fields::{scalar_ty, synthetic_field};
    vec![
        synthetic_field("limit", scalar_ty("Int", TypeArity::Optional)),
        synthetic_field("offset", scalar_ty("Int", TypeArity::Optional)),
        synthetic_field("fields", scalar_ty("String", TypeArity::List)),
        synthetic_field("include", scalar_ty("String", TypeArity::List)),
        synthetic_field("sort", scalar_ty("String", TypeArity::Optional)),
    ]
}
