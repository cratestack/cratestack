//! [`build_lock`] — the pure, no-I/O entry point. Takes a parsed schema and
//! an optional previously-committed lock, returns the lock that should now
//! be on disk. Callers (ticket #169's CLI) own reading the old lock and
//! writing the new one; this function never touches the filesystem.

use std::collections::BTreeSet;

use cratestack_core::Schema;

use super::assign::{build_enum_lock, build_message_lock};
use super::{EnumLock, MessageLock, PbLock, PbLockError};

/// Builds the lock a schema should have, given its (optional) previous
/// lock.
///
/// Message coverage for this ticket is deliberately narrow: every `model`
/// and every `type` declaration becomes a lock entry (both become proto
/// messages downstream), plus tombstones carried forward from `existing`
/// for names no longer in the schema at all. `Create<M>Input` /
/// `Update<M>Input` — the macro-synthesized per-model input messages
/// ticket #169 will also need numbers for — are **not** built here: their
/// field sets come from selection/macro logic this crate would otherwise
/// have to duplicate. Ticket #169 is expected to call `build_lock` once per
/// synthesized message shape it can materialize as a field list, using the
/// same `MessageLock`/algorithm — not to extend this function with
/// name-guessing.
pub fn build_lock(schema: &Schema, existing: Option<&PbLock>) -> Result<PbLock, PbLockError> {
    let version = existing.map(|lock| lock.version).unwrap_or(1);
    let package = existing.and_then(|lock| lock.package.clone());

    let mut message_names: BTreeSet<String> = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
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
            });
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
