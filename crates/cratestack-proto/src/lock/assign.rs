//! The per-message and per-enum assignment/reservation algorithm —
//! `docs/design/protobuf.md` §3.3, applied one message/enum at a time.
//!
//! The core invariant this module exists to hold: a number, once it has
//! ever been in `reserved`, is never handed to anything else again — not
//! by auto-assignment, and not by a colliding `@pb(N)` pin. A message or
//! enum that disappears from the schema entirely keeps its lock entry as a
//! tombstone (empty live map, everything it ever held moved to `reserved`)
//! rather than being dropped, so a same-named message/enum reintroduced
//! later cannot silently reuse a number that meant something else on the
//! wire.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::Field;

use super::numbering::assign_one;
use super::pin::pb_pin;
use super::{EnumLock, MessageLock, PbLockError};
use crate::casing::to_screaming_snake_case;

pub(super) fn build_message_lock(
    owner: &str,
    current_fields: Option<&[Field]>,
    existing: Option<&MessageLock>,
) -> Result<MessageLock, PbLockError> {
    let existing_fields = existing.map(|lock| &lock.fields);
    let existing_reserved = existing.map(|lock| lock.reserved.as_slice()).unwrap_or(&[]);

    let Some(fields) = current_fields else {
        return Ok(tombstone(existing_fields, existing_reserved));
    };

    let current_names: BTreeSet<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    let (mut numbers, mut reserved) =
        carry_forward(existing_fields, existing_reserved, &current_names);

    for field in fields {
        assign_one(
            owner,
            &field.name,
            pb_pin(owner, field)?,
            &mut numbers,
            &mut reserved,
        )?;
    }

    Ok(MessageLock {
        fields: numbers,
        reserved: reserved.into_iter().collect(),
    })
}

pub(super) fn build_enum_lock(
    owner: &str,
    current_variant_names: Option<&[String]>,
    existing: Option<&EnumLock>,
) -> Result<EnumLock, PbLockError> {
    let existing_variants = existing.map(|lock| &lock.variants);
    let existing_reserved = existing.map(|lock| lock.reserved.as_slice()).unwrap_or(&[]);

    let Some(declared) = current_variant_names else {
        return Ok(EnumLock {
            variants: BTreeMap::new(),
            reserved: tombstone(existing_variants, existing_reserved).reserved,
        });
    };

    let unspecified = format!("{}_UNSPECIFIED", to_screaming_snake_case(owner));
    let mut current_names: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
    current_names.insert(unspecified.as_str());

    let (mut numbers, mut reserved) =
        carry_forward(existing_variants, existing_reserved, &current_names);

    // The synthetic zero value always exists and is always 0 — never
    // shifted by declaration order, never freed, never reassigned.
    numbers.insert(unspecified, 0);

    for name in declared {
        assign_one(owner, name, None, &mut numbers, &mut reserved)?;
    }

    Ok(EnumLock {
        variants: numbers,
        reserved: reserved.into_iter().collect(),
    })
}

/// A message/enum absent from the current schema but present in the old
/// lock: everything it ever held moves to `reserved` and the live map goes
/// empty, rather than the entry disappearing.
fn tombstone(
    existing_fields: Option<&BTreeMap<String, i32>>,
    existing_reserved: &[i32],
) -> MessageLock {
    let mut reserved: BTreeSet<i32> = existing_reserved.iter().copied().collect();
    if let Some(fields) = existing_fields {
        reserved.extend(fields.values().copied());
    }
    MessageLock {
        fields: BTreeMap::new(),
        reserved: reserved.into_iter().collect(),
    }
}

/// Numbers still alive (their name survives into the current schema) carry
/// forward unchanged; numbers whose name did not survive move into
/// `reserved`, permanently.
fn carry_forward(
    existing: Option<&BTreeMap<String, i32>>,
    existing_reserved: &[i32],
    current_names: &BTreeSet<&str>,
) -> (BTreeMap<String, i32>, BTreeSet<i32>) {
    let mut reserved: BTreeSet<i32> = existing_reserved.iter().copied().collect();
    let mut numbers = BTreeMap::new();
    if let Some(existing) = existing {
        for (name, &number) in existing {
            if current_names.contains(name.as_str()) {
                numbers.insert(name.clone(), number);
            } else {
                reserved.insert(number);
            }
        }
    }
    (numbers, reserved)
}
