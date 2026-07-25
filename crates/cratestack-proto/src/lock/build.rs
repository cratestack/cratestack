//! [`build_lock`] — the pure, no-I/O entry point. Takes a parsed schema and
//! an optional previously-committed lock, returns the lock that should now
//! be on disk. Callers (ticket #169's CLI) own reading the old lock and
//! writing the new one; this function never touches the filesystem.

use std::collections::BTreeSet;

use cratestack_core::{Field, Schema};

use super::assign::{build_enum_lock, build_message_lock};
use super::{EnumLock, MessageLock, PbLock, PbLockError};

/// Builds the lock a schema should have, given its (optional) previous
/// lock.
///
/// Message coverage: every `model` and every `type` declaration becomes a
/// lock entry (both become proto messages downstream), plus whatever
/// `extra_messages` supplies — ticket #169's synthesized `Create<M>Input` /
/// `Update<M>Input` / `<Procedure>Input` / `<Procedure>Output` /
/// `PageOf<Item>` shapes, built from real `cratestack_core::Field` values
/// by the caller (their field-selection semantics belong with the emitter,
/// not here — this function only knows how to number a field list, not how
/// to derive one). Tombstones carry forward from `existing` for any name
/// —model, type, or synthesized— no longer supplied at all.
pub fn build_lock(
    schema: &Schema,
    existing: Option<&PbLock>,
    extra_messages: &std::collections::BTreeMap<String, Vec<Field>>,
) -> Result<PbLock, PbLockError> {
    let version = existing.map(|lock| lock.version).unwrap_or(1);
    let package = existing.and_then(|lock| lock.package.clone());

    let mut message_names: BTreeSet<String> = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
        .chain(extra_messages.keys().cloned())
        .collect();
    if let Some(existing) = existing {
        message_names.extend(existing.messages.keys().cloned());
    }

    let mut messages = std::collections::BTreeMap::new();
    for name in message_names {
        let current_fields = schema
            .models
            .iter()
            .find(|model| model.name == name)
            .map(|model| model.fields.as_slice())
            .or_else(|| {
                schema
                    .types
                    .iter()
                    .find(|ty| ty.name == name)
                    .map(|ty| ty.fields.as_slice())
            })
            .or_else(|| extra_messages.get(&name).map(|fields| fields.as_slice()));
        let existing_lock = existing.and_then(|lock| lock.messages.get(&name));
        let built: MessageLock = build_message_lock(&name, current_fields, existing_lock)?;
        messages.insert(name, built);
    }

    let mut enum_names: BTreeSet<String> =
        schema.enums.iter().map(|decl| decl.name.clone()).collect();
    if let Some(existing) = existing {
        enum_names.extend(existing.enums.keys().cloned());
    }

    let mut enums = std::collections::BTreeMap::new();
    for name in enum_names {
        let current_variants = schema
            .enums
            .iter()
            .find(|decl| decl.name == name)
            .map(|decl| {
                decl.variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect::<Vec<_>>()
            });
        let existing_lock = existing.and_then(|lock| lock.enums.get(&name));
        let built: EnumLock = build_enum_lock(&name, current_variants.as_deref(), existing_lock)?;
        enums.insert(name, built);
    }

    Ok(PbLock {
        version,
        package,
        messages,
        enums,
    })
}
