use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model, TypeArity, TypeRef};

// `is_computed_field`/`computed_params_fields`/
// `has_parameterized_computed_fields` live in `crate::computed_params`
// (split out per the repo's 200-LoC file convention) and are re-exported
// here so every existing `use crate::types::{...}` call site keeps
// working unchanged.
pub(crate) use crate::computed_params::{
    computed_params_fields, has_parameterized_computed_fields, is_computed_field,
};

pub(crate) fn ts_type(type_ref: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        return format!("Page<{}>", ts_type(item, enum_names));
    }

    if type_ref.is_find_many() {
        let item = type_ref
            .find_many_item()
            .expect("validated FindMany<T> should include an item type");
        // Unlike `Page<T>`, always a declared model (parser-enforced) —
        // maps straight to that model's own generated `<Model>FindMany`
        // (`crate::views::build_find_many_interface`), not a shared
        // generic wrapper: the `Where`/`OrderByClause` shape is
        // inherently per-model.
        return format!("{}FindMany", item.name);
    }

    let base = match type_ref.name.as_str() {
        "String" | "Cuid" | "Uuid" | "DateTime" => "string".to_owned(),
        // cratestack#498: a real arbitrary-precision `decimal.js` value
        // (`Decimal`, exported by `models.ts.j2` — see that template's
        // own doc comment), not a bare `string`. A `Decimal`-typed field
        // is *carried* as a wire-format string (both `decimal-rust-decimal`
        // and `decimal-bigdecimal` — #495/#496 — serialize it that way;
        // `find_many_views.rs` still maps it to `DecimalFilter` =
        // `ComparableFilter<Decimal>`), but the string's own *format*
        // depends on which backend built the server (`rust_decimal` never
        // emits scientific notation, `bigdecimal` does past a magnitude
        // threshold) — a bare `string` field type forced every consumer
        // to know that and hand-roll parsing.
        "Decimal" => "Decimal".to_owned(),
        "Int" | "Float" => "number".to_owned(),
        "Boolean" => "boolean".to_owned(),
        "Json" => "JsonValue".to_owned(),
        // cratestack#783 follow-up: a real `Uint8Array`, in *both*
        // directions, matching what the Dart client has always done
        // (`cratestack-client-dart`'s `dart_types.rs` maps `Bytes` to
        // `Uint8List` and converts at the wire boundary in
        // `wire_encode.rs`/`wire_decode.rs`). A `Bytes` field still
        // *travels* as an array of integers — the server's outbound shape
        // is unchanged — so the conversion happens in the generated
        // runtime: `encodeBinaryAsJson` on the way out (JSON transports
        // only; the native CBOR codec takes the `Uint8Array` directly and
        // emits a byte string), and the `bytesKeys`/`bytesListKeys` arms
        // of `models.ts.j2`'s shape walk on the way back.
        //
        // Deliberately not `Uint8Array | number[]`: a union would have to
        // be narrowed by every *reader*, and it cannot be applied
        // consistently anyway — a `type` block is a single generated
        // interface that can sit in an argument position, a return
        // position, or both (`procedure seal(env: Envelope): Envelope`),
        // so there is no input-only place to widen it. One type in both
        // directions has no such ambiguity.
        // EWKB bytes (cratestack#842) — same wire shape as `Bytes`.
        "Bytes" | "Geography" | "Geometry" => "Uint8Array".to_owned(),
        other if enum_names.contains(other) => other.to_owned(),
        other => other.to_owned(),
    };

    match type_ref.arity {
        TypeArity::Required => base,
        TypeArity::Optional => format!("{base} | null"),
        TypeArity::List => format!("{base}[]"),
    }
}

/// The type name a field's `TypeRef` actually names, unwrapping `Page<T>`
/// to `T` — the same unwrap `ts_type` does internally, factored out so
/// `crate::swr::ownership` and `crate::swr::context` (issue #304) can
/// classify a field's *referenced name* without re-deriving the full TS
/// type string `ts_type` produces.
pub(crate) fn base_type_name(type_ref: &TypeRef) -> &str {
    match type_ref.page_item() {
        Some(item) => base_type_name(item),
        None => &type_ref.name,
    }
}

pub(crate) fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
}

pub(crate) fn enum_name_set(enums: &[EnumDecl]) -> BTreeSet<&str> {
    enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect()
}

pub(crate) fn scalar_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field) && !is_server_only_field(field))
        .collect()
}

/// Fields visible on the generated model interface: everything except
/// `@server_only`. Unlike [`scalar_model_fields`], relation fields
/// stay in — the model interface (unlike Create/Update inputs) does
/// project relations.
pub(crate) fn visible_model_fields(model: &Model) -> Vec<&Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_server_only_field(field))
        .collect()
}

fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

/// Field carries `@server_only` — masked from outbound JSON, so it
/// must never appear in a generated client's model/Create/Update
/// interfaces.
fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Model has at least one `@@allow("create", ...)` or
/// `@@allow("all", ...)` rule. Mirrors the create verb's policy gate —
/// a model without one fail-closes on the server, so the generated
/// client shouldn't expose a `.create()` that can only ever 403.
pub(crate) fn model_allows_create(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .filter_map(|attribute| allow_action(&attribute.raw))
        .any(|action| action == "create" || action == "all")
}

fn allow_action(raw: &str) -> Option<&str> {
    let inner = raw.trim().strip_prefix("@@allow(")?;
    let quote = inner.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &inner[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

pub(crate) fn primary_key_field(model: &Model) -> Option<&Field> {
    model.fields.iter().find(|field| is_primary_key(field))
}

pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

pub(crate) fn is_generated_on_create(field: &Field) -> bool {
    has_default(field)
}

pub(crate) fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

/// The model's `@version` field, if it declares one — the optimistic-
/// concurrency column the server matches `If-Match` against. Exact-match
/// on `@version` (not `starts_with`, unlike `is_primary_key`'s `@id`)
/// because `@version` takes no arguments; the parser rejects any other
/// spelling. Matches how every other crate finds it
/// (`cratestack-macros::model::descriptor`, `cratestack-studio::data::model_info`).
///
/// Only `crate::refine` reads this today: the generated REST client sends
/// `If-Match` from a caller-supplied header, so nothing else in this
/// crate needs to know which field carries the version.
pub(crate) fn version_field(model: &Model) -> Option<&Field> {
    model.fields.iter().find(|field| {
        field
            .attributes
            .iter()
            .any(|attribute| attribute.raw == "@version")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_core::SourceSpan;

    fn type_ref(name: &str, arity: TypeArity) -> TypeRef {
        TypeRef {
            name: name.to_owned(),
            name_span: SourceSpan {
                start: 0,
                end: 0,
                line: 1,
            },
            arity,
            generic_args: Vec::new(),
            int_args: Vec::new(),
            ident_args: Vec::new(),
        }
    }

    fn rendered(name: &str, arity: TypeArity) -> String {
        ts_type(&type_ref(name, arity), &BTreeSet::new())
    }

    #[test]
    fn bytes_is_a_uint8_array_at_every_arity() {
        // The headline of cratestack#783's follow-up. Pinned here because
        // every other check of it is indirect (a snapshot, or the vitest
        // round-trip suite), and this is the one line a well-meaning
        // "TypeScript prefers plain arrays" change would flip back.
        assert_eq!(rendered("Bytes", TypeArity::Required), "Uint8Array");
        assert_eq!(rendered("Bytes", TypeArity::Optional), "Uint8Array | null");
        assert_eq!(rendered("Bytes", TypeArity::List), "Uint8Array[]");
    }

    #[test]
    fn an_int_list_stays_a_plain_number_array() {
        // `Int[]` and `Bytes` are the same `number[]` on the wire, so the
        // two must stay visibly different in the generated types — the
        // type-level half of the same distinction
        // `crate::wire_shapes` draws for decode-time revival.
        assert_eq!(rendered("Int", TypeArity::List), "number[]");
        assert_ne!(
            rendered("Int", TypeArity::List),
            rendered("Bytes", TypeArity::List)
        );
    }
}
