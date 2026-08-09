//! Schema-wide, path-aware `Decimal` field revival shapes (cratestack#498
//! F2/F5, cratestack#499 review remediation).
//!
//! ## History: why this isn't the flat, name-keyed scheme it started as
//!
//! The first cut of this module (`DecimalReachability`) computed, per root
//! type, the full transitive set of `Decimal` field *names* reachable
//! through relations/`type` blocks, and fed that flat `Set<string>` to
//! `reviveDecimalFieldsInner`'s generic "walk any nested object/array,
//! convert a string at any key in the set" runtime. That closed the
//! original gap (a relation-embedded `Decimal` field wasn't revived at
//! all) but reopened exactly the hazard its own doc comment warned about:
//! once two *different* reachable types can each contribute field names to
//! the same flat set, a non-`Decimal` field in one type that happens to
//! share a name with a `Decimal` field in another reachable type gets
//! wrongly matched — proven empirically (`tests/fixtures/
//! decimal_name_collision.cstack`, `tests/decimal_collision_regression.rs`):
//! an `Order.total: Decimal` + related `Account.total: String` schema,
//! `include`-ing the relation, throws `[DecimalError] Invalid argument]`
//! decoding a real (non-numeric) account reference, and silently corrupts
//! a numeric-looking one (`"00123"` -> `Decimal("123")`, losing the
//! leading zeros).
//!
//! ## The fix: revive by structural path, not by bare field name
//!
//! [`DecimalShapes`] builds one [`DecimalShapeView`] per `Model`/`TypeDecl`
//! in the schema — its own *direct* `Decimal` field names, plus a
//! field-name -> target-type-name map for any field whose type is itself
//! another model/`type`. Nothing is flattened or unioned across types.
//! `models.ts.j2`'s revival runtime looks up a shape by name and only
//! checks *that type's own* keys against *that type's own* decoded
//! object's properties; a nested field routes to *its own* type's shape
//! (via the `nested` map) rather than being checked against the parent's
//! key set. Two different types can now both have a field named `total`
//! with different types with zero collision risk, because "is `total`
//! meant to be a `Decimal` here" is answered per-type, not once globally.
//!
//! No reachability walk or cycle guard is needed here (unlike the old
//! `DecimalReachability`): each shape only describes its own type's
//! *direct* fields. Recursion happens at *runtime*, by shape-name lookup,
//! so a self- or mutually-referential relation is naturally handled — the
//! lookup just resolves the same shape again, with no eager pre-walk to
//! bound.
use std::collections::BTreeSet;

use cratestack_core::{Field, Schema, TypeRef};
use serde::Serialize;

/// One row of the generated `decimalShapes` registry (`models.ts.j2`) —
/// see this module's doc comment for the scheme.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DecimalShapeView {
    /// The model/`type` name this shape describes — the registry key.
    pub(crate) name: String,
    /// This type's own direct `Decimal` field wire names, as a JS
    /// array-literal source fragment (e.g. `['amount']`, or `[]`).
    pub(crate) keys_js: String,
    /// This type's own direct relation-/`type`-typed fields, as a JS
    /// object-literal source fragment mapping wire field name to the
    /// target type's shape name (e.g. `{ 'customer': 'Customer' }`, or
    /// `{}`). The runtime looks up `decimalShapes[targetName]` at decode
    /// time — see `models.ts.j2`'s `reviveShaped`.
    pub(crate) nested_js: String,
}

/// Builds one [`DecimalShapeView`] per model/`type` in `schema`, in a
/// stable (schema declaration) order.
pub(crate) fn build_decimal_shapes(schema: &Schema) -> Vec<DecimalShapeView> {
    let model_and_type_names: BTreeSet<&str> = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .chain(schema.types.iter().map(|ty| ty.name.as_str()))
        .collect();

    let mut shapes = Vec::with_capacity(schema.models.len() + schema.types.len());
    for model in &schema.models {
        shapes.push(build_shape(
            &model.name,
            &model.fields,
            &model_and_type_names,
        ));
    }
    for ty in &schema.types {
        shapes.push(build_shape(&ty.name, &ty.fields, &model_and_type_names));
    }
    shapes
}

fn build_shape(
    name: &str,
    fields: &[Field],
    model_and_type_names: &BTreeSet<&str>,
) -> DecimalShapeView {
    let mut keys = Vec::new();
    let mut nested = Vec::new();
    for field in fields {
        if is_server_only(field) {
            // Masked from outbound JSON (`@server_only`) — never appears
            // in a decoded response, so it needs no revival entry either
            // way.
            continue;
        }
        if field.ty.name == "Decimal" {
            keys.push(field.name.clone());
        } else if model_and_type_names.contains(field.ty.name.as_str()) {
            nested.push((field.name.clone(), field.ty.name.clone()));
        }
    }
    DecimalShapeView {
        name: name.to_owned(),
        keys_js: crate::views::js_string_array(&keys),
        nested_js: js_string_map(&nested),
    }
}

fn is_server_only(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Renders `entries` (wire field name, target shape name) as a JS
/// object-literal source fragment with single-quoted string keys *and*
/// values (e.g. `{ 'author': 'User', 'editor': 'User' }`, or `{}` for an
/// empty slice) — mirrors `views::js_string_array`'s quoting convention.
/// The key is quoted (not a bare identifier, unlike `FieldView::property`
/// elsewhere in this crate): a bare identifier can't losslessly represent
/// every valid wire field name (a schema field can be a reserved word like
/// `class`, which is valid as a *quoted* JS property key but would need
/// the wrong kind of escaping — variable-name mangling, not string
/// escaping — to survive as a bare one), and this key is looked up at
/// runtime via `shape.nested[key]`, an ordinary string-indexed lookup that
/// doesn't care whether the key was written bare or quoted.
fn js_string_map(entries: &[(String, String)]) -> String {
    let body = entries
        .iter()
        .map(|(key, value)| {
            format!(
                "'{}': '{}'",
                crate::naming::escape_ts_string(key),
                crate::naming::escape_ts_string(value)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {body} }}")
}

/// How a procedure's return type should be revived on decode
/// (cratestack#498 F2) — see `ProcedureView::decimal_revival_kind`'s doc
/// comment (`crate::views`) for the generated-code side of this.
pub(crate) enum ProcedureDecimalRevival {
    /// The return type is (optionally list/optional) `Decimal` itself —
    /// `Page<Decimal>` is not a validated shape (`Page<T>`'s item must be a
    /// declared model or `type`, `cratestack-parser::validate::type_names`),
    /// so this can only ever be `Decimal`, `Decimal?`, or `Decimal[]`.
    Scalar,
    /// The (`Page<T>`-unwrapped) base type names a declared model or
    /// `type` — decode via that type's own shape (`String` is the shape
    /// name; a name with no registry entry, e.g. a plain scalar or enum
    /// return, is `reviveDecimalFields`'s documented no-op fast path).
    /// `paged` is `true` when the original return type was `Page<T>` —
    /// the generated call site uses `revivePagedDecimalFields` instead of
    /// `reviveDecimalFields` for that case (the decoded envelope's own
    /// keys, `items`/`totalCount`/`pageInfo`, are never themselves `T`'s
    /// fields, so `T`'s shape must be applied to `.items`, not the
    /// envelope).
    Shape { shape_name: String, paged: bool },
}

/// Classifies `return_type` per [`ProcedureDecimalRevival`].
pub(crate) fn procedure_decimal_revival(return_type: &TypeRef) -> ProcedureDecimalRevival {
    let paged = return_type.is_page();
    let base = return_type.page_item().unwrap_or(return_type);
    if base.name == "Decimal" {
        ProcedureDecimalRevival::Scalar
    } else {
        ProcedureDecimalRevival::Shape {
            shape_name: base.name.clone(),
            paged,
        }
    }
}

#[cfg(test)]
#[path = "decimal_tests.rs"]
mod tests;
