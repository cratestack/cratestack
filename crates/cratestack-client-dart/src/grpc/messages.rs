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
use crate::config::DartGeneratorError;
use crate::idents::to_camel_case;
use crate::naming::visible_model_fields;

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
) -> Result<(), DartGeneratorError> {
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
/// (`PageInfo`, `PageOf<M>`) have no pre-existing schema field of their
/// own to match — the hand-written `PageInfo`/`Page<T>` classes in
/// `models.dart.j2` use camelCase Dart map keys (`hasNextPage`,
/// `pageInfo`, `totalCount`) rather than the synthetic snake_case field
/// names (`has_next_page`) this collector otherwise seeds them with, so
/// this flag renames the *decoded-map key* to match. Every other message
/// (model/type/Create/Update/Rpc*Input) keeps the field's real name
/// unchanged — that must match `models.dart.j2`'s generated
/// `wire_name` (`crate::builders::build_data_class`'s `FieldView::wire_name
/// = field.name.clone()`, unescaped) so the `Map<String, Object?>` this
/// generator's runtime decodes off the wire can be handed straight to the
/// already-generated `<Message>.fromWire(map)` factory.
pub(super) fn collect_from_fields(
    name: &str,
    fields: &[Field],
    pb_lock: &PbLock,
    enum_names: &BTreeSet<&str>,
    schema: &Schema,
    camel_case_properties: bool,
    seen: &mut Seen,
) -> Result<(), DartGeneratorError> {
    if seen.contains_key(name) {
        return Ok(());
    }
    let numbers =
        pb_lock
            .messages
            .get(name)
            .ok_or_else(|| DartGeneratorError::MissingPbLockEntry {
                message: name.to_owned(),
                field: String::new(),
            })?;

    let mut descriptors = Vec::with_capacity(fields.len());
    let mut nested = Vec::new();
    for field in fields {
        let number = *numbers.fields.get(&field.name).ok_or_else(|| {
            DartGeneratorError::MissingPbLockEntry {
                message: name.to_owned(),
                field: format!(" field `{}`", field.name),
            }
        })?;
        let property = if camel_case_properties {
            to_camel_case(&field.name)
        } else {
            field.name.clone()
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
