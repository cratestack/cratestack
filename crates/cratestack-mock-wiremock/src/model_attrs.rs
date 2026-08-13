//! Field/model attribute predicates this crate needs for the same
//! reason `crates/cratestack-macros/src/shared/attrs.rs` has them —
//! whether a field is the primary key, is excluded from the client
//! projection, or a model paginates its `list` route. Not imported from
//! `cratestack-macros` (its predicates are `pub(crate)` to that crate,
//! and pulling in `cratestack-macros` as a dependency here would be a
//! much heavier edge than three one-line string checks against
//! `Attribute::raw` warrant) — mirrored instead, deliberately kept this
//! small so drift from the real predicates stays easy to spot in review.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, Schema, TypeArity};

/// Field carries an `@id`-prefixed attribute — the model's primary key.
pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

/// Field carries `@server_only` — never serialized to a client, so
/// excluded from a synthesized record the same way
/// `crates/cratestack-macros/src/axum/model/serializers/
/// projection_fields.rs`'s default projection excludes it.
pub(crate) fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Model carries `@@paged` — its `list` route returns the
/// `{items, totalCount, pageInfo}` envelope instead of a bare array.
pub(crate) fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

/// Field carries `@version` — the optimistic-lock column. Mirrors
/// `crates/cratestack-macros/src/shared/attrs.rs`'s predicate of the
/// same name; parser validation (`cratestack-parser/src/validate/
/// model_attributes.rs`) guarantees at most one per model and that it's
/// a required `Int`, so this generator never needs to re-check either
/// of those — only whether the attribute is present at all.
pub(crate) fn is_version_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@version")
}

/// Field's type names another declared model — a relation field,
/// populated only via `include=<relation>` and excluded from the
/// default projection this generator synthesizes.
pub(crate) fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

/// How a field's value round-trips through the WireMock stub's hand-
/// assembled JSON text (`crates/cratestack-mock-wiremock/src/
/// model_state/`) — whether the rendered Handlebars fragment needs to be
/// wrapped in literal quotes to stay valid JSON, or is a bare JSON
/// number/boolean. Only `Required`-arity scalars this generator knows
/// how to echo/store through `wiremock-state-extension` classify as
/// [`ScalarKind::Number`]/[`ScalarKind::Bool`]/[`ScalarKind::QuotedString`]
/// — everything else (`Optional`/`List` arity, `Json`/`Bytes`/`Vector`,
/// or a nested `type`/enum-as-object) is [`ScalarKind::Unsupported`] and
/// falls back to a frozen, non-stateful example value instead (see
/// `docs/design/wiremock-stubs.md`'s "Model CRUD statefulness" section
/// for why: the extension's per-record state store only round-trips
/// scalar leaf properties, not arbitrary nested JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarKind {
    Number,
    Bool,
    QuotedString,
    Unsupported,
}

pub(crate) fn classify_field_kind(schema: &Schema, field: &Field) -> ScalarKind {
    if field.ty.arity != TypeArity::Required {
        return ScalarKind::Unsupported;
    }
    match field.ty.name.as_str() {
        "Int" | "Float" => ScalarKind::Number,
        "Boolean" => ScalarKind::Bool,
        "String" | "Cuid" | "Uuid" | "DateTime" => ScalarKind::QuotedString,
        other => {
            // An enum variant renders as a plain JSON string (its variant
            // name), same wire shape as `String` — echoing/storing it
            // through the state store is exactly as safe as any other
            // string-kind field. Anything else (a nested `type`, `Json`,
            // `Bytes`, `Vector(n)`) is unsupported.
            if schema.enums.iter().any(|e| e.name == other) {
                ScalarKind::QuotedString
            } else {
                ScalarKind::Unsupported
            }
        }
    }
}
