use std::collections::BTreeMap;

use cratestack_core::{Procedure, ProcedureArg, Schema, TypeArity};

use super::arity::{arity_label, classify_arity_change};
use super::{Change, Severity};

pub(super) fn diff_procedures(prev: &Schema, next: &Schema, changes: &mut Vec<Change>) {
    let prev_by_name = index(&prev.procedures);
    let next_by_name = index(&next.procedures);

    for name in prev_by_name.keys() {
        if !next_by_name.contains_key(name) {
            changes.push(Change {
                severity: Severity::Breaking,
                category: "procedure_removed",
                subject: format!("procedure `{name}`"),
                message: format!("procedure `{name}` was removed"),
            });
        }
    }

    for name in next_by_name.keys() {
        if !prev_by_name.contains_key(name) {
            changes.push(Change {
                severity: Severity::Additive,
                category: "procedure_added",
                subject: format!("procedure `{name}`"),
                message: format!("procedure `{name}` was added"),
            });
        }
    }

    for (name, prev_proc) in &prev_by_name {
        let Some(next_proc) = next_by_name.get(name) else {
            continue;
        };
        diff_matched_procedure(name, prev_proc, next_proc, changes);
    }
}

fn index(procedures: &[Procedure]) -> BTreeMap<&str, &Procedure> {
    procedures
        .iter()
        .map(|proc| (proc.name.as_str(), proc))
        .collect()
}

fn diff_matched_procedure(
    name: &str,
    prev: &Procedure,
    next: &Procedure,
    changes: &mut Vec<Change>,
) {
    if prev.kind != next.kind {
        changes.push(Change {
            severity: Severity::Breaking,
            category: "procedure_kind_changed",
            subject: format!("procedure `{name}`"),
            message: format!(
                "procedure `{name}` kind changed from {:?} to {:?} (query/mutation dispatch differs)",
                prev.kind, next.kind
            ),
        });
    }

    if prev.return_type.name != next.return_type.name {
        changes.push(Change {
            severity: Severity::Breaking,
            category: "procedure_return_type_changed",
            subject: format!("procedure `{name}`"),
            message: format!(
                "procedure `{name}` return type changed from `{}` to `{}`",
                prev.return_type.name, next.return_type.name
            ),
        });
    } else if prev.return_type.arity != next.return_type.arity {
        let severity = classify_arity_change(prev.return_type.arity, next.return_type.arity);
        changes.push(Change {
            severity,
            category: "procedure_return_arity_changed",
            subject: format!("procedure `{name}`"),
            message: format!(
                "procedure `{name}` return type arity changed from {} to {}",
                arity_label(prev.return_type.arity),
                arity_label(next.return_type.arity)
            ),
        });
    }

    diff_args(name, &prev.args, &next.args, changes);
}

fn diff_args(
    proc_name: &str,
    prev: &[ProcedureArg],
    next: &[ProcedureArg],
    changes: &mut Vec<Change>,
) {
    let prev_by_name = index_args(prev);
    let next_by_name = index_args(next);

    for name in prev_by_name.keys() {
        if !next_by_name.contains_key(name) {
            changes.push(Change {
                severity: Severity::Breaking,
                category: "procedure_arg_removed",
                subject: format!("{proc_name}({name})"),
                message: format!("procedure `{proc_name}` lost argument `{name}`"),
            });
        }
    }

    for (name, next_arg) in &next_by_name {
        match prev_by_name.get(name) {
            None => push_added_arg(changes, proc_name, name, next_arg),
            Some(prev_arg) => diff_matched_arg(proc_name, name, prev_arg, next_arg, changes),
        }
    }
}

fn index_args(args: &[ProcedureArg]) -> BTreeMap<&str, &ProcedureArg> {
    args.iter().map(|arg| (arg.name.as_str(), arg)).collect()
}

fn push_added_arg(changes: &mut Vec<Change>, proc_name: &str, name: &str, arg: &ProcedureArg) {
    if arg.ty.arity == TypeArity::Required {
        changes.push(Change {
            severity: Severity::Breaking,
            category: "procedure_arg_added_required",
            subject: format!("{proc_name}({name})"),
            message: format!(
                "procedure `{proc_name}` gained required argument `{name}` \
                 — existing callers omitting it will fail"
            ),
        });
        return;
    }

    changes.push(Change {
        severity: Severity::Additive,
        category: "procedure_arg_added",
        subject: format!("{proc_name}({name})"),
        message: format!("procedure `{proc_name}` gained optional argument `{name}`"),
    });
}

fn diff_matched_arg(
    proc_name: &str,
    name: &str,
    prev: &ProcedureArg,
    next: &ProcedureArg,
    changes: &mut Vec<Change>,
) {
    if prev.ty.name != next.ty.name {
        changes.push(Change {
            severity: Severity::Breaking,
            category: "procedure_arg_retyped",
            subject: format!("{proc_name}({name})"),
            message: format!(
                "procedure `{proc_name}` argument `{name}` type changed from `{}` to `{}`",
                prev.ty.name, next.ty.name
            ),
        });
        return;
    }

    if prev.ty.arity != next.ty.arity {
        let severity = classify_arity_change(prev.ty.arity, next.ty.arity);
        changes.push(Change {
            severity,
            category: "procedure_arg_arity_changed",
            subject: format!("{proc_name}({name})"),
            message: format!(
                "procedure `{proc_name}` argument `{name}` arity changed from {} to {}",
                arity_label(prev.ty.arity),
                arity_label(next.ty.arity)
            ),
        });
    }
}
