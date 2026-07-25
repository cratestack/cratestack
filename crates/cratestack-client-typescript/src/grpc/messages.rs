//! The recursive message-descriptor collector: given a message name and
//! its field list, looks up field numbers in the schema's `.pb.lock`,
//! builds a [`GrpcMessageView`], and follows any message-typed field
//! reference (a relation, or a `type` block) to whatever depth the schema
//! actually uses — deduplicated by name so a diamond or self-referencing
//! relation graph is visited once. [`super::crud_messages`] is what seeds
//! the worklist this collector walks (`Widget`, `WidgetRpcListInput`,
//! `PageOfWidget`, ...); this module doesn't know about CRUD verbs at
//! all, only "message name -> field list -> nested message names".
//!
//! Field *numbers* always come from the committed `<schema>.pb.lock`
//! (never invented here) — see `docs/design/protobuf.md` §3.3. A missing
//! lock entry is a hard error (`MissingPbLockEntry`): it means the lock is
//! stale relative to the schema, which `cratestack generate-proto --check`
//! is the tool for catching, not something this generator should paper
//! over by guessing a number.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Schema};
use cratestack_proto::PbLock;
use serde::Serialize;

use super::wire::{GrpcWireKind, build_field_descriptor};
use crate::naming::to_camel_case;
use crate::templates::TypeScriptGeneratorError;
use crate::types::visible_model_fields;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GrpcMessageView {
    pub(crate) name: String,
    pub(crate) fields: Vec<super::wire::GrpcFieldDescriptor>,
}

pub(super) type Seen = BTreeMap<String, GrpcMessageView>;

pub(super) fn collect_message(
    name: &str,
    schema: &Schema,
    pb_lock: &PbLock,
    enum_names: &BTreeSet<&str>,
    seen: &mut Seen,
) -> Result<(), TypeScriptGeneratorError> {
    if seen.contains_key(name) {
        return Ok(());
    }
    if let Some(model) = schema.models.iter().find(|model| model.name == name) {
        let fields = visible_model_fields(model)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        return collect_from_fields(name, &fields, pb_lock, enum_names, schema, false, seen);
    }
    if let Some(ty) = schema.types.iter().find(|ty| ty.name == name) {
        let fields = ty.fields.clone();
        return collect_from_fields(name, &fields, pb_lock, enum_names, schema, false, seen);
    }
    // Neither a model nor a `type` block — an enum reference (no message
    // to build) or a name a validated schema wouldn't have let through
    // unresolved. Either way, nothing to recurse into.
    Ok(())
}

/// Looks up field numbers in `pb_lock`, builds this message's field
/// descriptors, and recurses into any message-typed field it references.
///
/// `camel_case_properties`: the framework-synthesized helper messages
/// (`PageInfo`, `PageOf<M>`) have no pre-existing `models.ts` interface to
/// match, so this generator gives their TS properties the same camelCase
/// convention `models.ts`'s hand-authored `Page<T>`/`PageInfo` already
/// use (`pageInfo`, `hasNextPage`, ...) — the lock lookup still uses the
/// real (snake_case) field name, only the generated TS property is
/// renamed. Every other message (model/type/Create/Update/Rpc*Input) uses
/// the field's real name unchanged, because its TS property must match
/// the interface `models.ts` already generated for it via the shared,
/// transport-agnostic `context::build_template_context` path.
pub(super) fn collect_from_fields(
    name: &str,
    fields: &[Field],
    pb_lock: &PbLock,
    enum_names: &BTreeSet<&str>,
    schema: &Schema,
    camel_case_properties: bool,
    seen: &mut Seen,
) -> Result<(), TypeScriptGeneratorError> {
    if seen.contains_key(name) {
        return Ok(());
    }
    let numbers =
        pb_lock
            .messages
            .get(name)
            .ok_or_else(|| TypeScriptGeneratorError::MissingPbLockEntry {
                message: name.to_owned(),
                field: String::new(),
            })?;

    let mut descriptors = Vec::with_capacity(fields.len());
    let mut nested = Vec::new();
    for field in fields {
        let number = *numbers.fields.get(&field.name).ok_or_else(|| {
            TypeScriptGeneratorError::MissingPbLockEntry {
                message: name.to_owned(),
                field: format!(" field `{}`", field.name),
            }
        })?;
        let property = if camel_case_properties {
            to_camel_case(&field.name)
        } else {
            crate::naming::ts_identifier(&field.name)
        };
        let descriptor = build_field_descriptor(&property, &field.ty, number, enum_names);
        if descriptor.kind == GrpcWireKind::Message
            && let Some(ref_name) = &descriptor.ref_name
        {
            nested.push(ref_name.clone());
        }
        descriptors.push(descriptor);
    }

    seen.insert(
        name.to_owned(),
        GrpcMessageView {
            name: name.to_owned(),
            fields: descriptors,
        },
    );

    for ref_name in nested {
        collect_message(&ref_name, schema, pb_lock, enum_names, seen)?;
    }
    Ok(())
}
