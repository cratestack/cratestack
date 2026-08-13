//! `.proto` text emission — ticket #169 Parts B/C. Messages and enums only;
//! no `service` block (that's ticket #170, which needs a third
//! `TransportStyle` variant that doesn't exist yet).
//!
//! [`synthesize_messages`] is Part B: it derives the field lists for every
//! message this crate's [`crate::build_lock`] doesn't already cover from
//! `schema.models`/`schema.types` alone (`Create<M>Input`,
//! `Update<M>Input`, `<Procedure>Input`, `<Procedure>Output`,
//! `PageOf<Item>`, `PageInfo`). [`emit_proto`] is Part C: it turns a schema
//! plus an already-built [`crate::PbLock`] plus that same synthesized-field
//! map into `.proto` source text, reading field/variant numbers from the
//! lock rather than recomputing them.
//!
//! Callers (ticket #169's CLI) are expected to call `synthesize_messages`
//! once, feed its output to both `build_lock` (as `extra_messages`) and
//! `emit_proto`, in that order — see `docs/design/protobuf.md` §5 and the
//! ticket's Part D pseudocode.

mod enum_render;
mod error;
mod field;
mod header;
mod message;
mod mirror;
mod rpc_input_synth;
mod scalar;
mod service;
mod synth;
mod synth_page;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_grpc;

use std::collections::BTreeMap;

use cratestack_core::{Field, Schema, TransportStyle};

pub use error::ProtoEmitError;
pub use synth::synthesize_messages;
pub use synth_page::monomorphize_return_type;

use crate::PbLock;
use mirror::{visible_model_fields, visible_type_fields};

pub fn emit_proto(
    schema: &Schema,
    lock: &PbLock,
    extra_messages: &BTreeMap<String, Vec<Field>>,
    schema_path: &str,
) -> Result<String, ProtoEmitError> {
    let package = lock
        .package
        .as_deref()
        .ok_or(ProtoEmitError::MissingPackage)?;

    let mut bodies: BTreeMap<String, Vec<&Field>> = BTreeMap::new();
    for model in &schema.models {
        bodies.insert(model.name.clone(), visible_model_fields(model));
    }
    for ty in &schema.types {
        bodies.insert(ty.name.clone(), visible_type_fields(ty));
    }
    for (name, fields) in extra_messages {
        bodies.insert(name.clone(), fields.iter().collect());
    }

    let mut enums_by_name: BTreeMap<&str, &cratestack_core::EnumDecl> = BTreeMap::new();
    for enum_decl in &schema.enums {
        enums_by_name.insert(enum_decl.name.as_str(), enum_decl);
    }

    let mut needs_timestamp_import = false;
    let mut enum_blocks = Vec::with_capacity(enums_by_name.len());
    for (name, decl) in &enums_by_name {
        let enum_lock = lock
            .enums
            .get(*name)
            .ok_or_else(|| ProtoEmitError::MissingEnumLock((*name).to_owned()))?;
        enum_blocks.push(enum_render::render_enum(decl, enum_lock)?);
    }

    let mut message_blocks = Vec::with_capacity(bodies.len());
    for (name, fields) in &bodies {
        let message_lock = lock
            .messages
            .get(name)
            .ok_or_else(|| ProtoEmitError::MissingMessageLock(name.clone()))?;
        let rendered = message::render_message(name, fields, &message_lock.fields)?;
        needs_timestamp_import |= rendered.needs_timestamp_import;
        message_blocks.push(rendered.text);
    }

    // `service` block: ticket #170, `transport grpc` schemas only — a
    // `rest`/`rpc` schema's `.proto` stays messages-and-enums-only exactly
    // as ticket #169 shipped it.
    let service_block = if schema.transport == TransportStyle::Grpc {
        let methods = service::build_service_methods(schema, extra_messages);
        Some(service::render_service(&methods))
    } else {
        None
    };

    Ok(header::render_file(
        schema,
        schema_path,
        package,
        needs_timestamp_import,
        &enum_blocks,
        &message_blocks,
        service_block.as_deref(),
    ))
}
