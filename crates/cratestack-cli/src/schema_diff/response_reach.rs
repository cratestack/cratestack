//! The declarations a response can carry: every model and view (their read
//! routes return them), every procedure's and query's result type, and
//! everything their fields reach, through `type`s and generic arguments.
//!
//! An enum in this set is one an old client decodes, so adding a variant to
//! it breaks that client; an enum outside it only ever arrives in requests.
//! Deliberately conservative: a `@server_only` field, or a model whose read
//! route is suppressed, still counts as reachable.

use std::collections::BTreeSet;

use cratestack_core::{Field, Schema, TypeRef};

pub(super) fn response_reachable(schema: &Schema) -> BTreeSet<&str> {
    let mut pending: Vec<&str> = Vec::new();
    pending.extend(schema.models.iter().map(|m| m.name.as_str()));
    pending.extend(schema.views.iter().map(|v| v.name.as_str()));
    for procedure in &schema.procedures {
        type_names(&procedure.return_type, &mut pending);
    }
    for query in &schema.queries {
        type_names(&query.result_type, &mut pending);
    }

    let mut reached = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !reached.insert(name) {
            continue;
        }
        for field in fields_of(schema, name) {
            type_names(&field.ty, &mut pending);
        }
    }
    reached
}

fn type_names<'a>(ty: &'a TypeRef, out: &mut Vec<&'a str>) {
    out.push(&ty.name);
    for arg in &ty.generic_args {
        type_names(arg, out);
    }
}

fn fields_of<'a>(schema: &'a Schema, name: &str) -> &'a [Field] {
    if let Some(model) = schema.models.iter().find(|m| m.name == name) {
        return &model.fields;
    }
    if let Some(view) = schema.views.iter().find(|v| v.name == name) {
        return &view.fields;
    }
    if let Some(ty) = schema.types.iter().find(|t| t.name == name) {
        return &ty.fields;
    }
    &[]
}
