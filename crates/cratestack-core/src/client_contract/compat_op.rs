//! The op-level half of the classifier (see `compat`): identity, arguments,
//! return type, and which directions each declaration is reachable in.

use std::collections::BTreeSet;

use super::owned::{Arg, OpContract, TypeRef};

/// The declarations reachable from the op's input roots and from its output
/// roots, taken over both contracts (the wider of the two views decides).
pub(super) struct Directions {
    pub(super) input: BTreeSet<String>,
    pub(super) output: BTreeSet<String>,
}

impl Directions {
    pub(super) fn new(old: &OpContract, new: &OpContract) -> Self {
        let mut input = BTreeSet::new();
        let mut output = BTreeSet::new();
        for op in [old, new] {
            let (inputs, outputs) = roots(op);
            input.extend(op.closure.reach(&inputs));
            output.extend(op.closure.reach(&outputs));
        }
        Self { input, output }
    }

    pub(super) fn input_only(&self, name: &str) -> bool {
        self.input.contains(name) && !self.output.contains(name)
    }

    pub(super) fn output_only(&self, name: &str) -> bool {
        self.output.contains(name) && !self.input.contains(name)
    }
}

/// `(input roots, output roots)`. A model op's model is both: its verb
/// decides which, but the filter, the `create` body and the row all share
/// its fields, so each rule set applies.
fn roots(op: &OpContract) -> (Vec<&str>, Vec<&str>) {
    if let Some(model) = &op.model {
        return (vec![model], vec![model]);
    }
    let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
    if let Some(procedure) = &op.procedure {
        procedure.args.iter().for_each(|a| a.ty.names(&mut inputs));
        procedure.return_type.names(&mut outputs);
    }
    (inputs, outputs)
}

/// `Int?`, `Widget[]`, `Page<Widget>`: a type as a person writes it.
pub(super) fn show(ty: &TypeRef) -> String {
    let mut out = ty.name.clone();
    if !ty.generic_args.is_empty() {
        let args: Vec<String> = ty.generic_args.iter().map(show).collect();
        out = format!("{out}<{}>", args.join(", "));
    }
    match ty.arity.as_str() {
        "optional" => out + "?",
        "list" => out + "[]",
        _ => out,
    }
}

/// `old` is required and `new` is the same type, optional.
pub(super) fn widens(old: &TypeRef, new: &TypeRef) -> bool {
    old.arity == "required"
        && new.arity == "optional"
        && *old
            == TypeRef {
                arity: "required".to_owned(),
                ..new.clone()
            }
}

fn same<T: PartialEq + std::fmt::Debug>(what: &str, old: &T, new: &T, reasons: &mut Vec<String>) {
    if old != new {
        reasons.push(format!("{what} changed from {old:?} to {new:?}"));
    }
}

pub(super) fn op(old: &OpContract, new: &OpContract, reasons: &mut Vec<String>) {
    same("the transport", &old.transport, &new.transport, reasons);
    same("the op key", &old.key, &new.key, reasons);
    same("the op kind", &old.kind, &new.kind, reasons);
    same("the op verb", &old.verb, &new.verb, reasons);
    same("the model", &old.model, &new.model, reasons);
    same("the emitted events", &old.events, &new.events, reasons);
    match (&old.procedure, &new.procedure) {
        (None, None) => {}
        (Some(old), Some(new)) => {
            same("the procedure name", &old.name, &new.name, reasons);
            same("the procedure kind", &old.kind, &new.kind, reasons);
            if old.return_type != new.return_type {
                reasons.push(format!(
                    "the return type changed from {} to {}",
                    show(&old.return_type),
                    show(&new.return_type)
                ));
            }
            attributes("the procedure", &old.attributes, &new.attributes, reasons);
            args(&old.args, &new.args, reasons);
        }
        _ => reasons.push("the op changed between a model op and a procedure".to_owned()),
    }
}

/// Attributes are compared as sets: declaration order is not a shape.
pub(super) fn attributes(what: &str, old: &[String], new: &[String], reasons: &mut Vec<String>) {
    let (old, new): (BTreeSet<_>, BTreeSet<_>) = (old.iter().collect(), new.iter().collect());
    for removed in old.difference(&new) {
        reasons.push(format!("{what} lost the attribute {removed}"));
    }
    for added in new.difference(&old) {
        reasons.push(format!("{what} gained the attribute {added}"));
    }
}

fn args(old: &[Arg], new: &[Arg], reasons: &mut Vec<String>) {
    for arg in old {
        let Some(now) = new.iter().find(|a| a.name == arg.name) else {
            reasons.push(format!(
                "the argument `{}` was removed: an old client's value would be ignored",
                arg.name
            ));
            continue;
        };
        if now.ty != arg.ty && !widens(&arg.ty, &now.ty) {
            reasons.push(format!(
                "the argument `{}` changed from {} to {}",
                arg.name,
                show(&arg.ty),
                show(&now.ty)
            ));
        }
    }
    let names = |list: &'_ [Arg], other: &'_ [Arg]| -> Vec<String> {
        list.iter()
            .filter(|a| other.iter().any(|b| b.name == a.name))
            .map(|a| a.name.clone())
            .collect()
    };
    if names(old, new) != names(new, old) {
        reasons.push("the declared order of the existing arguments changed".to_owned());
    }
    for added in new.iter().filter(|a| old.iter().all(|o| o.name != a.name)) {
        if added.ty.arity != "optional" {
            reasons.push(format!(
                "the new argument `{}` is {}, not optional: an old client cannot send it",
                added.name,
                show(&added.ty)
            ));
        }
    }
}
