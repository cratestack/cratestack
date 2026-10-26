//! Ordinal enums across a diff (`docs/design/enum-ordinals.md` §3.2, §4):
//! the refusals that keep an enum's ordinals append-only, and the
//! name ↔ ordinal map a column switching storage needs.
//!
//! The history lives in [`Projections::enums`], so it is only as long as
//! one snapshot step. `@@reserved` is what makes a retirement durable: a
//! number freed three migrations ago is invisible otherwise.

use std::collections::BTreeMap;

use crate::error::MigrateError;
use crate::ir::{ColumnType, EnumCast, Op};
use crate::projection::{EnumProjection, Projections};

/// Refuses a diff that rewrites an ordinal enum's history. An enum that
/// was a string enum, or did not exist, on the previous side has no
/// history to check.
pub(crate) fn check_ordinal_history(
    prev: &Projections,
    next: &Projections,
) -> Result<(), MigrateError> {
    for (name, was) in &prev.enums {
        let Some(now) = next.enums.get(name) else {
            continue;
        };
        check_enum(name, was, now)?;
    }
    Ok(())
}

fn check_enum(name: &str, was: &EnumProjection, now: &EnumProjection) -> Result<(), MigrateError> {
    let refuse = |reason: String| {
        Err(MigrateError::EnumOrdinalHistory {
            enum_name: name.to_owned(),
            reason,
        })
    };
    let now_by_name: BTreeMap<&str, i64> =
        now.variants.iter().map(|(n, o)| (n.as_str(), *o)).collect();
    let now_by_ordinal: BTreeMap<i64, &str> =
        now.variants.iter().map(|(n, o)| (*o, n.as_str())).collect();

    for (variant, ordinal) in &was.variants {
        if let Some(renumbered) = now_by_name.get(variant.as_str())
            && renumbered != ordinal
        {
            return refuse(format!(
                "the ordinal of `{variant}` changed from {ordinal} to {renumbered}; add a new variant instead"
            ));
        }
        if !now_by_ordinal.contains_key(ordinal) && !now.reserved.contains(ordinal) {
            return refuse(format!(
                "`{variant}` (ordinal {ordinal}) was removed without retiring its number; add \
                 `@@reserved({ordinal})` to the enum"
            ));
        }
    }
    for retired in &was.reserved {
        if !now.reserved.contains(retired) {
            return refuse(format!(
                "ordinal {retired} was dropped from `@@reserved`; a retired number stays retired"
            ));
        }
        if let Some(variant) = now_by_ordinal.get(retired) {
            return refuse(format!(
                "`{variant}` uses ordinal {retired}, which an earlier migration retired with `@@reserved`"
            ));
        }
    }
    Ok(())
}

/// Gives each `AlterColumnType` that switches an enum between names and
/// ordinals the map its `USING CASE` needs, from whichever side is ordinal.
pub(crate) fn attach_enum_casts(ops: &mut [Op], prev: &Projections, next: &Projections) {
    for op in ops {
        let Op::AlterColumnType(alter) = op else {
            continue;
        };
        let cast = match (&alter.from, &alter.to) {
            (ColumnType::Enum(_), ColumnType::OrdinalEnum(name)) => {
                next.enums.get(name).map(|e| (e.variants.clone(), true))
            }
            (ColumnType::OrdinalEnum(name), ColumnType::Enum(_)) => {
                prev.enums.get(name).map(|e| (e.variants.clone(), false))
            }
            _ => None,
        };
        alter.enum_cast = cast.map(|(pairs, to_ordinals)| EnumCast { pairs, to_ordinals });
    }
}
