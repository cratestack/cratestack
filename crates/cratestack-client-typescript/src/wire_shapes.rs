//! Schema-wide, path-aware field revival shapes: which of a type's own
//! fields need converting out of their wire form on decode, and into what
//! (cratestack#498 F2/F5, cratestack#499 review remediation, plus `Bytes`
//! per cratestack#783's follow-up — see the last section).
//!
//! Named `wire_shapes` rather than `decimal` because it now describes two
//! conversions, not one; the history below is `Decimal`'s, but every
//! argument in it applies unchanged to `Bytes`.
//!
//! ## History: why this isn't the flat, name-keyed scheme it started as
//!
//! The first cut of this module (`DecimalReachability`) computed, per root
//! type, the full transitive set of `Decimal` field *names* reachable
//! through relations/`type` blocks, and fed that flat `Set<string>` to
//! the revival runtime's generic "walk any nested object/array,
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
//! [`build_wire_shapes`] builds one [`WireShapeView`] per `Model`/`TypeDecl`
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
//!
//! ## Why this carries `Bytes` too, and why it *has* to
//!
//! A schema `Bytes` field is a `Uint8Array` on both sides of a generated
//! TypeScript client, but it travels as an array of integers on the wire
//! (the server's outbound shape, deliberately unchanged by cratestack#783).
//! Reviving it therefore needs exactly the same machinery `Decimal` needs,
//! and for exactly the same reason: **the wire form is not
//! self-identifying**. A decoded `[1, 2, 3]` is indistinguishable from an
//! `Int[]` field's value, just as a decoded `"1.5"` is indistinguishable
//! from a `String` field's — so "is this field `Bytes` here" can only be
//! answered per-type, from the schema, which is what this registry is.
//! Piggy-backing on the existing shape walk means one traversal converts
//! both, and the collision-safety argument above covers `Bytes` unchanged.
//!
//! `Bytes` needs one thing `Decimal` does not: its keys are split by
//! **arity**. A `Decimal` leaf is a string and a `Decimal[]` is an array
//! of strings, so the runtime can tell them apart structurally. A `Bytes`
//! leaf is `number[]` and a `Bytes[]` is `number[][]` — distinguishable
//! for a *populated* value, but `[]` is ambiguous: it is either an empty
//! `Uint8Array` or an empty list of them. Recording the arity here removes
//! the guess rather than letting the runtime infer it wrong at the one
//! input that matters least and surprises most.
use std::collections::BTreeSet;

use cratestack_core::{Field, Schema, TypeArity, TypeRef};
use serde::Serialize;

/// One row of the generated `wireShapes` registry (`models.ts.j2`) —
/// see this module's doc comment for the scheme.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WireShapeView {
    /// The model/`type` name this shape describes — the registry key.
    pub(crate) name: String,
    /// This type's own direct `Decimal` field wire names, as a JS
    /// array-literal source fragment (e.g. `['amount']`, or `[]`).
    pub(crate) decimal_keys_js: String,
    /// This type's own direct `Bytes` field wire names at `Required`/
    /// `Optional` arity — each decodes from a wire integer array into one
    /// `Uint8Array`.
    pub(crate) bytes_keys_js: String,
    /// This type's own direct `Bytes` field wire names at `List` arity —
    /// each decodes from a wire array *of* integer arrays into
    /// `Uint8Array[]`. Kept separate from [`Self::bytes_keys_js`] because
    /// an empty `[]` cannot be classified structurally; see the module
    /// doc.
    pub(crate) bytes_list_keys_js: String,
    /// This type's own direct relation-/`type`-typed fields, as a JS
    /// object-literal source fragment mapping wire field name to the
    /// target type's shape name (e.g. `{ 'customer': 'Customer' }`, or
    /// `{}`). The runtime looks up `wireShapes[targetName]` at decode
    /// time — see `models.ts.j2`'s `reviveShaped`.
    pub(crate) nested_js: String,
}

/// Builds one [`WireShapeView`] per model/`type` in `schema`, in a
/// stable (schema declaration) order.
pub(crate) fn build_wire_shapes(schema: &Schema) -> Vec<WireShapeView> {
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
) -> WireShapeView {
    let mut decimal_keys = Vec::new();
    let mut bytes_keys = Vec::new();
    let mut bytes_list_keys = Vec::new();
    let mut nested = Vec::new();
    for field in fields {
        if is_server_only(field) {
            // Masked from outbound JSON (`@server_only`) — never appears
            // in a decoded response, so it needs no revival entry either
            // way.
            continue;
        }
        if field.ty.name == "Decimal" {
            decimal_keys.push(field.name.clone());
        } else if field.ty.name == "Bytes" {
            // Split by arity — see the module doc for why `[]` cannot be
            // classified structurally at runtime.
            if matches!(field.ty.arity, TypeArity::List) {
                bytes_list_keys.push(field.name.clone());
            } else {
                bytes_keys.push(field.name.clone());
            }
        } else if model_and_type_names.contains(field.ty.name.as_str()) {
            nested.push((field.name.clone(), field.ty.name.clone()));
        }
    }
    WireShapeView {
        name: name.to_owned(),
        decimal_keys_js: crate::views::js_string_array(&decimal_keys),
        bytes_keys_js: crate::views::js_string_array(&bytes_keys),
        bytes_list_keys_js: crate::views::js_string_array(&bytes_list_keys),
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
/// (cratestack#498 F2) — see `ProcedureView::revival_kind`'s doc
/// comment (`crate::procedure_views`) for the generated-code side of this.
pub(crate) enum ProcedureRevival {
    /// The return type is a bare scalar needing revival — `Decimal` or
    /// `Bytes`, at any arity. `Page<T>` is not a validated shape for
    /// either (`Page<T>`'s item must be a declared model or `type`,
    /// `cratestack-parser::validate::type_names`), so this can only ever
    /// be the bare name, `?`, or `[]`.
    Scalar(ScalarRevival),
    /// The (`Page<T>`-unwrapped) base type names a declared model or
    /// `type` — decode via that type's own shape (`String` is the shape
    /// name; a name with no registry entry, e.g. a plain scalar or enum
    /// return, is `reviveWireFields`'s documented no-op fast path).
    /// `paged` is `true` when the original return type was `Page<T>` —
    /// the generated call site uses `revivePagedWireFields` instead of
    /// `reviveWireFields` for that case (the decoded envelope's own
    /// keys, `items`/`totalCount`/`pageInfo`, are never themselves `T`'s
    /// fields, so `T`'s shape must be applied to `.items`, not the
    /// envelope).
    Shape { shape_name: String, paged: bool },
}

/// Which bare-scalar revival a [`ProcedureRevival::Scalar`] return needs.
/// Rendered straight into the generated `reviveWireScalar(value, "...")`
/// call as its second argument, so these strings are a wire contract with
/// `models.ts.j2`'s runtime — keep the two in lockstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarRevival {
    /// `Decimal` / `Decimal?` / `Decimal[]` — a wire string (or array of
    /// them) becomes a `Decimal` instance.
    Decimal,
    /// `Bytes` / `Bytes?` — one wire integer array becomes one
    /// `Uint8Array`.
    Bytes,
    /// `Bytes[]` — a wire array *of* integer arrays becomes
    /// `Uint8Array[]`. Distinct from [`Self::Bytes`] for the same
    /// arity-ambiguity reason the field-level keys are split; see the
    /// module doc.
    BytesList,
}

impl ScalarRevival {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ScalarRevival::Decimal => "decimal",
            ScalarRevival::Bytes => "bytes",
            ScalarRevival::BytesList => "bytesList",
        }
    }
}

/// Classifies `return_type` per [`ProcedureRevival`].
pub(crate) fn procedure_revival(return_type: &TypeRef) -> ProcedureRevival {
    let paged = return_type.is_page();
    let base = return_type.page_item().unwrap_or(return_type);
    match base.name.as_str() {
        "Decimal" => ProcedureRevival::Scalar(ScalarRevival::Decimal),
        "Bytes" if matches!(base.arity, TypeArity::List) => {
            ProcedureRevival::Scalar(ScalarRevival::BytesList)
        }
        "Bytes" => ProcedureRevival::Scalar(ScalarRevival::Bytes),
        _ => ProcedureRevival::Shape {
            shape_name: base.name.clone(),
            paged,
        },
    }
}

#[cfg(test)]
#[path = "wire_shapes_tests.rs"]
mod tests;
