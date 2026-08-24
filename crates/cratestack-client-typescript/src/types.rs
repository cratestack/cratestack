use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model, TypeArity, TypeRef, computed_params_type_name};

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
        "Bytes" => "number[]".to_owned(),
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

/// Field carries `@computed`/`@computed(params: <Type>?)`
/// (`docs/design/computed-fields.md`) — resolved at response time, never
/// stored. Unlike a relation or `@server_only` field, a computed field
/// IS part of the default model projection (`scalar_model_fields`/
/// `visible_model_fields` deliberately do NOT exclude it — a computed
/// field is exactly as "scalar" as any other leaf field from the wire's
/// point of view), so call sites that need to exclude it (create/update
/// inputs, `Where`/`SortField` builders) check this explicitly.
pub(crate) fn is_computed_field(field: &Field) -> bool {
    cratestack_core::is_computed_field(field)
}

/// The model's own `@computed(params: <Type>?)` fields, paired with each
/// one's declared params type name — the source data for the generated
/// `<Model>ComputedParams` interface (`crate::views::build_computed_params_interface`).
/// A bare `@computed` field (no `params:` argument) never contributes here:
/// [`cratestack_core::computed_params_type_name`] returns `None` for it,
/// same as for a field with no `@computed` attribute at all.
pub(crate) fn computed_params_fields(model: &Model) -> Vec<(&Field, &str)> {
    model
        .fields
        .iter()
        .filter_map(|field| {
            computed_params_type_name(field).map(|params_type| (field, params_type))
        })
        .collect()
}

/// Whether `model` declares at least one *parameterized*
/// `@computed(params: <Type>?)` field — not merely `@computed` in general.
/// Gates the generated `get`/`list` `computedParams` surface per model: the
/// server 422s a `computedParams` key that doesn't name a parameterized
/// field, so a model whose only computed fields are bare (or with none at
/// all) must not accept the parameter in the first place — it could never
/// be satisfied. Mirrors `cratestack-client-dart`'s
/// `ModelApiView::has_parameterized_computed_fields` predicate exactly
/// (same server-side reasoning, different client).
pub(crate) fn has_parameterized_computed_fields(model: &Model) -> bool {
    !computed_params_fields(model).is_empty()
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
