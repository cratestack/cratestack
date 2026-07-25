//! The low-level number bookkeeping `assign.rs` drives one field/variant at
//! a time: honor a pin, keep a carried-forward number as-is, or hand out
//! the next free one — skipping protobuf's own reserved range.

use std::collections::{BTreeMap, BTreeSet};

use super::{PROTO_RESERVED_RANGE, PbLockError};

pub(super) fn assign_one(
    owner: &str,
    name: &str,
    pin: Option<i32>,
    numbers: &mut BTreeMap<String, i32>,
    reserved: &mut BTreeSet<i32>,
) -> Result<(), PbLockError> {
    match pin {
        Some(number) => {
            if numbers.get(name) == Some(&number) {
                return Ok(()); // already pinned to this exact number; no-op
            }
            check_collision(owner, name, number, numbers, reserved)?;
            if let Some(old) = numbers.insert(name.to_owned(), number) {
                reserved.insert(old);
            }
        }
        None => {
            if !numbers.contains_key(name) {
                let next = next_number(numbers, reserved);
                numbers.insert(name.to_owned(), next);
            }
        }
    }
    Ok(())
}

fn check_collision(
    owner: &str,
    field: &str,
    number: i32,
    numbers: &BTreeMap<String, i32>,
    reserved: &BTreeSet<i32>,
) -> Result<(), PbLockError> {
    if reserved.contains(&number) {
        return Err(PbLockError::PinCollidesWithReserved {
            owner: owner.to_owned(),
            field: field.to_owned(),
            number,
        });
    }
    if numbers.iter().any(|(existing_name, &existing_number)| {
        existing_name != field && existing_number == number
    }) {
        return Err(PbLockError::PinCollidesWithUsed {
            owner: owner.to_owned(),
            field: field.to_owned(),
            number,
        });
    }
    Ok(())
}

fn next_number(numbers: &BTreeMap<String, i32>, reserved: &BTreeSet<i32>) -> i32 {
    let highest = numbers
        .values()
        .copied()
        .chain(reserved.iter().copied())
        .max()
        .unwrap_or(0);
    let candidate = highest + 1;
    if PROTO_RESERVED_RANGE.contains(&candidate) {
        PROTO_RESERVED_RANGE.end() + 1
    } else {
        candidate
    }
}
