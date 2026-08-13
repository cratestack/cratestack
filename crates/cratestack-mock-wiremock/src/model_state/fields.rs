//! Splits a model's non-relation, non-`@server_only` fields into the
//! two buckets the stateful generator treats differently: fields the
//! `wiremock-state-extension` state store can round-trip
//! ([`StateField`] — echoed on create/update, read back on get/list/
//! delete), and fields it can't ([`FrozenField`] — a fixed example
//! value, same on every response, computed once via `values::synthesize`
//! exactly like the pre-stateful static generator did).

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, Schema};

use crate::error::WireMockGeneratorError;
use crate::model_attrs::{
    ScalarKind, classify_field_kind, is_relation_field, is_server_only_field,
};
use crate::values::synthesize;

pub(crate) struct StateField {
    pub(crate) name: String,
    pub(crate) kind: ScalarKind,
    /// Unquoted default text (e.g. `string`, `0`, `true`) used as the
    /// create-time fallback when the client's create body omits this
    /// field — the same per-scalar-type default `values.rs` uses,
    /// duplicated here in unquoted form because [`super::fragments::
    /// quote_wrap`] is what adds quoting, once, at assembly time.
    pub(crate) default_literal: String,
}

pub(crate) struct FrozenField {
    pub(crate) name: String,
    /// Already-serialized JSON text for this field's fixed example value
    /// (e.g. `"string"` or `[]`) — ready to splice directly into a
    /// hand-assembled body string, no further quoting needed.
    pub(crate) literal_json: String,
}

pub(crate) struct ModelFieldPlan {
    pub(crate) pk_name: String,
    pub(crate) pk_type_name: String,
    pub(crate) pk_kind: ScalarKind,
    pub(crate) stateful: Vec<StateField>,
    pub(crate) frozen: Vec<FrozenField>,
}

/// Builds the field plan for `model`. Errors only for the same defense-
/// in-depth reasons `values::synthesize` already can (an unvalidated
/// `&Schema` referencing an unknown type, or an unbreakable cycle in a
/// frozen field's own type graph) — never for a field simply being
/// [`ScalarKind::Unsupported`], which is an expected, handled case, not
/// a failure.
pub(crate) fn build_field_plan(
    schema: &Schema,
    model: &Model,
    model_names: &BTreeSet<&str>,
    pk_field: &Field,
) -> Result<ModelFieldPlan, WireMockGeneratorError> {
    let owner = format!("model `{}`", model.name);
    let mut stateful = Vec::new();
    let mut frozen = Vec::new();

    for field in &model.fields {
        if field.name == pk_field.name
            || is_relation_field(model_names, field)
            || is_server_only_field(field)
        {
            continue;
        }
        match classify_field_kind(schema, field) {
            ScalarKind::Unsupported => {
                let mut in_progress = vec![model.name.clone()];
                let value = synthesize(schema, &owner, &field.ty, &mut in_progress)?;
                frozen.push(FrozenField {
                    name: field.name.clone(),
                    literal_json: value.to_string(),
                });
            }
            kind => stateful.push(StateField {
                name: field.name.clone(),
                kind,
                default_literal: scalar_default_literal(schema, &field.ty.name),
            }),
        }
    }

    Ok(ModelFieldPlan {
        pk_name: pk_field.name.clone(),
        pk_type_name: pk_field.ty.name.clone(),
        pk_kind: classify_field_kind(schema, pk_field),
        stateful,
        frozen,
    })
}

/// The unquoted inner text of the fixed default `values.rs`'s
/// `synthesize_base` produces for each scalar type
/// [`crate::model_attrs::classify_field_kind`] accepts — kept in sync
/// with that function by hand (both are short, both change rarely, and
/// pulling `values.rs`'s `Value`-returning version here would just mean
/// stripping the quotes back off a `String` result).
fn scalar_default_literal(schema: &Schema, type_name: &str) -> String {
    match type_name {
        "Int" => "0".to_owned(),
        "Float" => "0.0".to_owned(),
        "Boolean" => "true".to_owned(),
        "Cuid" => "clxxxxxxxxxxxxxxxxxxxxxxxx".to_owned(),
        "Uuid" => "00000000-0000-0000-0000-000000000000".to_owned(),
        "DateTime" => "1970-01-01T00:00:00Z".to_owned(),
        "String" => "string".to_owned(),
        other => schema
            .enums
            .iter()
            .find(|e| e.name == other)
            .and_then(|e| e.variants.first())
            .map(|variant| variant.name.clone())
            // Unreachable for a field `classify_field_kind` accepted as
            // `QuotedString`/`Number`/`Bool` — defensive fallback only.
            .unwrap_or_else(|| "string".to_owned()),
    }
}
