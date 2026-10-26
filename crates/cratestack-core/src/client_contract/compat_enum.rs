//! The enum half of the declaration classifier (see `compat`). A string enum
//! travels as its variant names; an ordinal enum (`Pending = 1`,
//! `docs/design/enum-ordinals.md`) travels as its numbers, so for it a
//! rename is wire-neutral and a renumbering is not.

use super::compat_op::Directions;
use super::owned::Enum;

pub(super) fn enum_decl(old: &Enum, new: &Enum, dirs: &Directions, reasons: &mut Vec<String>) {
    match (wire_ordinals(old), wire_ordinals(new)) {
        (None, None) => by_name(old, new, dirs, reasons),
        (Some(was), Some(now)) => by_ordinal(&old.name, &was, &now, dirs, reasons),
        (was, _) => reasons.push(format!(
            "the enum `{}` changed from {} to {} on the wire",
            old.name,
            form(was.is_some()),
            form(was.is_none()),
        )),
    }
}

fn form(ordinal: bool) -> &'static str {
    if ordinal { "ordinals" } else { "names" }
}

/// The values an ordinal enum puts on the wire; `None` for a string enum.
fn wire_ordinals(e: &Enum) -> Option<Vec<i64>> {
    e.ordinals
        .as_ref()
        .map(|o| o.iter().flatten().copied().collect())
}

fn by_name(old: &Enum, new: &Enum, dirs: &Directions, reasons: &mut Vec<String>) {
    if old.variants == new.variants {
        return;
    }
    let appended = new.variants.starts_with(&old.variants);
    if !(appended && dirs.input_only(&old.name)) {
        reasons.push(format!(
            "the enum `{}` changed from [{}] to [{}]{}",
            old.name,
            old.variants.join(", "),
            new.variants.join(", "),
            if appended {
                ": a variant added to an enum an old client decodes is refused"
            } else {
                ""
            }
        ));
    }
}

/// Only the set of numbers matters: names are not on the wire, and order
/// is not either, since each value names itself.
fn by_ordinal(name: &str, was: &[i64], now: &[i64], dirs: &Directions, reasons: &mut Vec<String>) {
    let gone: Vec<String> = missing_from(was, now);
    let added: Vec<String> = missing_from(now, was);
    if !gone.is_empty() {
        reasons.push(format!(
            "the enum `{name}` no longer carries ordinal {}: an old client's value is refused",
            gone.join(", ")
        ));
    }
    if !added.is_empty() && !dirs.input_only(name) {
        reasons.push(format!(
            "the enum `{name}` gained ordinal {}: a value added to an enum an old client decodes is refused",
            added.join(", ")
        ));
    }
}

fn missing_from(these: &[i64], those: &[i64]) -> Vec<String> {
    these
        .iter()
        .filter(|n| !those.contains(n))
        .map(i64::to_string)
        .collect()
}
