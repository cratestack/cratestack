//! Records, in a stub mapping's `metadata`, the validators its placeholder
//! values could not satisfy (cratestack#1307). Only `String` fields can
//! end up here (see `field_value::string_value`); the best candidate is
//! still emitted, and each affected field is listed under
//! `metadata.cratestackUnsatisfiedValidators` as
//! `{ "path": "line.note", "validators": ["@regex(...)", "@length(...)"] }`,
//! `path` being the dotted field path from the response body root.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, Procedure, Schema, TypeRef};
use serde_json::{Value, json};

use crate::field_value::string_value;
use crate::model_attrs::{is_relation_field, is_server_only_field};
use crate::validator_spec::ValidatorSpec;

pub(crate) const METADATA_KEY: &str = "cratestackUnsatisfiedValidators";

pub(crate) struct Gap {
    path: String,
    validators: Vec<String>,
}

pub(crate) fn procedure_gaps(schema: &Schema, procedure: &Procedure) -> Vec<Gap> {
    let ty = procedure
        .return_type
        .page_item()
        .unwrap_or(&procedure.return_type);
    let mut gaps = Vec::new();
    walk_named(schema, ty, "", &mut Vec::new(), &mut gaps);
    gaps
}

pub(crate) fn model_gaps(schema: &Schema, model: &Model, names: &BTreeSet<&str>) -> Vec<Gap> {
    let mut gaps = Vec::new();
    let mut seen = vec![model.name.clone()];
    let fields = model
        .fields
        .iter()
        .filter(|f| !is_relation_field(names, f) && !is_server_only_field(f));
    walk_fields(schema, fields, "", &mut seen, &mut gaps);
    gaps
}

/// Adds the gaps to `mapping`'s existing `metadata` object (no-op if none).
pub(crate) fn attach(mapping: &mut Value, mut gaps: Vec<Gap>) {
    if gaps.is_empty() {
        return;
    }
    gaps.sort_by(|a, b| a.path.cmp(&b.path));
    let list: Vec<Value> = gaps
        .into_iter()
        .map(|g| json!({ "path": g.path, "validators": g.validators }))
        .collect();
    mapping["metadata"][METADATA_KEY] = Value::Array(list);
}

fn walk_named(
    schema: &Schema,
    ty: &TypeRef,
    prefix: &str,
    seen: &mut Vec<String>,
    gaps: &mut Vec<Gap>,
) {
    if seen.contains(&ty.name) {
        return;
    }
    let fields = schema
        .models
        .iter()
        .find(|m| m.name == ty.name)
        .map(|m| &m.fields)
        .or_else(|| {
            schema
                .types
                .iter()
                .find(|t| t.name == ty.name)
                .map(|t| &t.fields)
        });
    if let Some(fields) = fields {
        seen.push(ty.name.clone());
        walk_fields(schema, fields.iter(), prefix, seen, gaps);
        seen.pop();
    }
}

fn walk_fields<'a>(
    schema: &Schema,
    fields: impl Iterator<Item = &'a Field>,
    prefix: &str,
    seen: &mut Vec<String>,
    gaps: &mut Vec<Gap>,
) {
    for field in fields {
        let path = format!("{prefix}{}", field.name);
        let spec = ValidatorSpec::from_field(field);
        if field.ty.name == "String" && !spec.is_empty() {
            let (_, failed) = string_value(field, &spec);
            if !failed.is_empty() {
                gaps.push(Gap {
                    path,
                    validators: failed,
                });
            }
        } else {
            walk_named(schema, &field.ty, &format!("{path}."), seen, gaps);
        }
    }
}
