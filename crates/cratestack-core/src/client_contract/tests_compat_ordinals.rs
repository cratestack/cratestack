//! The classifier on ordinal enums (`compat_enum`): the wire carries the
//! numbers, so a rename or a reorder is compatible and a renumbering is not.
//! `Shade` is input-only and `Tone` output-only in `tests_compat::base()`.

use super::tests_compat::*;
use super::{Verdict, classify};
use crate::schema::Schema;

const PAINT: &str = "procedure.paint";
const TONE: [(&str, i64); 2] = [("Warm", 1), ("Cool", 2)];
const SHADE: [(&str, i64); 2] = [("Light", 1), ("Dark", 2)];

/// Give `name` exactly these variants, numbered.
fn number(s: &mut Schema, name: &str, variants: &[(&str, i64)]) {
    let names: Vec<&str> = variants.iter().map(|(n, _)| *n).collect();
    set_variants(s, name, &names);
    for (variant, (_, ordinal)) in enum_mut(s, name).variants.iter_mut().zip(variants) {
        variant.ordinal = Some(*ordinal);
    }
}

/// PAINT's verdict from `old(base())` to `new(base())`.
fn verdict(old: impl FnOnce(&mut Schema), new: impl FnOnce(&mut Schema)) -> Verdict {
    let (mut was, mut now) = (base(), base());
    old(&mut was);
    new(&mut now);
    classify(&contract(&was, PAINT), &contract(&now, PAINT))
}

#[track_caller]
fn has_reason(verdict: &Verdict, needle: &str) {
    assert!(
        verdict.reasons().iter().any(|r| r.contains(needle)),
        "wanted a reason with {needle:?}, got {verdict:?}"
    );
}

#[test]
fn renaming_or_reordering_ordinal_variants_is_compatible() {
    for now in [[("Hot", 1), ("Cool", 2)], [("Cool", 2), ("Warm", 1)]] {
        let v = verdict(|s| number(s, "Tone", &TONE), |s| number(s, "Tone", &now));
        assert!(v.is_compatible(), "{now:?}: {v:?}");
    }
}

#[test]
fn renumbering_or_dropping_an_ordinal_is_breaking() {
    let v = verdict(
        |s| number(s, "Tone", &TONE),
        |s| number(s, "Tone", &[("Warm", 1), ("Cool", 3)]),
    );
    has_reason(&v, "no longer carries ordinal 2");
    let v = verdict(
        |s| number(s, "Shade", &SHADE),
        |s| number(s, "Shade", &[("Light", 1)]),
    );
    has_reason(&v, "no longer carries ordinal 2");
}

#[test]
fn a_new_ordinal_breaks_only_an_enum_an_old_client_decodes() {
    let v = verdict(
        |s| number(s, "Tone", &TONE),
        |s| number(s, "Tone", &[("Warm", 1), ("Cool", 2), ("Hot", 3)]),
    );
    has_reason(&v, "gained ordinal 3");
    let v = verdict(
        |s| number(s, "Shade", &SHADE),
        |s| number(s, "Shade", &[("Light", 1), ("Dark", 2), ("Mid", 3)]),
    );
    assert!(v.is_compatible(), "{v:?}");
}

#[test]
fn switching_between_names_and_ordinals_is_breaking() {
    let v = verdict(|_| {}, |s| number(s, "Tone", &TONE));
    has_reason(&v, "changed from names to ordinals");
    let v = verdict(|s| number(s, "Tone", &TONE), |_| {});
    has_reason(&v, "changed from ordinals to names");
}
