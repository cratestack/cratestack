//! The declaration half of the classifier (see `compat`): every model,
//! type, view and enum of the old closure, judged by the directions it is
//! reachable in.

use super::attrs::attribute_name;
use super::compat_op::{Directions, attributes, show, widens};
use super::owned::{Decl, Enum, Field, Kind, OpContract};

pub(super) fn closure(old: &OpContract, new: &OpContract, reasons: &mut Vec<String>) {
    let dirs = Directions::new(old, new);
    for kind in Kind::ALL {
        for was in old.closure.decls(kind) {
            match new.closure.decls(kind).iter().find(|d| d.name == was.name) {
                Some(now) => decl(kind, was, now, &dirs, old.model.as_deref(), reasons),
                None => reasons.push(format!(
                    "the {} `{}` left the op's contract",
                    kind.word(),
                    was.name
                )),
            }
        }
    }
    for was in &old.closure.enums {
        match new.closure.enums.iter().find(|e| e.name == was.name) {
            Some(now) => enum_decl(was, now, &dirs, reasons),
            None => reasons.push(format!("the enum `{}` left the op's contract", was.name)),
        }
    }
}

fn decl(
    kind: Kind,
    old: &Decl,
    new: &Decl,
    dirs: &Directions,
    op_model: Option<&str>,
    reasons: &mut Vec<String>,
) {
    let name = format!("{} `{}`", kind.word(), old.name);
    attributes(&name, &old.attributes, &new.attributes, reasons);
    for was in &old.fields {
        let at = format!("`{}.{}`", old.name, was.name);
        match new.fields.iter().find(|f| f.name == was.name) {
            Some(now) => field(&at, was, now, dirs.input_only(&old.name), reasons),
            None => reasons.push(format!(
                "{at} was removed: an old peer still sends or reads it"
            )),
        }
    }
    for added in new
        .fields
        .iter()
        .filter(|n| old.fields.iter().all(|o| o.name != n.name))
    {
        // `@default` is honoured only by a model's own create input, which
        // leaves the field out; a `type`, or a model reached as a procedure
        // argument, decodes it as required and fails on the old message.
        let defaulted = matches!(kind, Kind::Model)
            && op_model == Some(old.name.as_str())
            && added
                .attributes
                .iter()
                .any(|a| attribute_name(a) == "@default");
        let admissible = dirs.output_only(&old.name) || added.ty.arity == "optional" || defaulted;
        if !admissible {
            reasons.push(format!(
                "the new field `{}.{}` is {} and is neither optional nor `@default` on the op's own model: an old client cannot send it",
                old.name,
                added.name,
                show(&added.ty)
            ));
        }
    }
}

fn field(at: &str, old: &Field, new: &Field, input_only: bool, reasons: &mut Vec<String>) {
    if old.ty != new.ty && !(input_only && widens(&old.ty, &new.ty)) {
        reasons.push(format!(
            "{at} changed from {} to {}",
            show(&old.ty),
            show(&new.ty)
        ));
    }
    attributes(at, &old.attributes, &new.attributes, reasons);
}

fn enum_decl(old: &Enum, new: &Enum, dirs: &Directions, reasons: &mut Vec<String>) {
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
