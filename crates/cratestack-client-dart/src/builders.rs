use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, TypeArity};

use crate::dart_types::dart_field_type;
use crate::idents::{dart_identifier, to_camel_case};
use crate::naming::is_relation_field;
use crate::views::{DataClassKind, DataClassView, EnumVariantView, EnumView, FieldView};
use crate::wire_decode::decode_value_expr;
use crate::wire_encode::encode_value_expr;

pub(crate) fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
    EnumView {
        name: enum_decl.name.clone(),
        variants: enum_decl
            .variants
            .iter()
            .map(|variant| EnumVariantView {
                identifier: dart_identifier(&to_camel_case(&variant.name)),
                wire_name: variant.name.clone(),
            })
            .collect(),
    }
}

/// Renders the exact text between `@CratestackBuilder(...)`'s parens (issue
/// #668 phase 2/3) — empty for the all-defaults case, so the template can
/// always write `@CratestackBuilder({{ builder_args }})` unconditionally
/// rather than branching per argument.
///
/// `touch_flag_fields`/`non_defaulting_list_fields` name FIELDS (not the
/// flags/setters themselves) — `package:cratestack_builder` derives the
/// rest structurally, mirroring `listDefaults`' own "one non-recoverable
/// argument" precedent.
fn render_builder_args(
    list_defaults: bool,
    touch_flag_fields: &[String],
    non_defaulting_list_fields: &[String],
) -> String {
    let mut parts = Vec::new();
    if !list_defaults {
        parts.push("listDefaults: false".to_owned());
    }
    if !touch_flag_fields.is_empty() {
        parts.push(format!(
            "touchFlagFields: {{{}}}",
            touch_flag_fields
                .iter()
                .map(|field| format!("'{field}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !non_defaulting_list_fields.is_empty() {
        parts.push(format!(
            "nonDefaultingListFields: {{{}}}",
            non_defaulting_list_fields
                .iter()
                .map(|field| format!("'{field}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join(", ")
}

pub(crate) fn build_data_class(
    name: &str,
    fields: &[&Field],
    kind: DataClassKind,
    enum_names: &BTreeSet<&str>,
    model_names: &BTreeSet<&str>,
) -> DataClassView {
    let field_views: Vec<FieldView> = fields
        .iter()
        .map(|field| {
            FieldView::new(
                dart_identifier(&field.name),
                field.name.clone(),
                dart_field_type(field, kind),
                matches!(kind, DataClassKind::Plain)
                    && matches!(field.ty.arity, TypeArity::Required | TypeArity::List),
                matches!(kind, DataClassKind::Patch),
                matches!(field.ty.arity, TypeArity::Optional),
                decode_value_expr(
                    &format!("value['{}']", field.name),
                    &field.ty,
                    enum_names,
                    matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel),
                    name,
                    &field.name,
                ),
                encode_value_expr(
                    &dart_identifier(&field.name),
                    &field.ty,
                    enum_names,
                    matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel),
                ),
            )
        })
        .collect();

    // `{field}IsSet` touch flags Rust actually synthesized (issue #668
    // phase 2/3) — passed explicitly rather than left for
    // `package:cratestack_builder` to recover by matching `bool` fields
    // named `{other}IsSet`, which fires on any ordinary user-declared field
    // shaped that way too (`cratestack-parser`'s
    // `tests_patch_touch_flag_collisions.rs` deliberately accepts a
    // non-nullable `weight` beside an unrelated `weightIsSet` field).
    let touch_flag_fields: Vec<String> = field_views
        .iter()
        .filter(|field| field.is_nullable_patch_field)
        .map(|field| field.identifier.clone())
        .collect();

    // A to-many *relation*-valued field on a model class (issue #661): Rust
    // builds that class from `scalar_model_fields`, which drops relation
    // fields entirely, so a Dart `addPosts`/`?? []` default there would
    // conflate "this relation was not included in the response" with
    // "included and empty" — no Rust builder counterpart, and a real
    // wire-visible divergence. Deliberately scoped to `ProjectionModel`
    // only: a `type` block's fields go through `scoped_builder_fields` on
    // the Rust side historically, which does NOT filter relations, so a
    // model-typed list inside a `type` keeps its normal list defaulting.
    let non_defaulting_list_fields: Vec<String> = fields
        .iter()
        .zip(field_views.iter())
        .filter(|(field, _)| {
            matches!(kind, DataClassKind::ProjectionModel)
                && field.ty.arity == TypeArity::List
                && is_relation_field(model_names, field)
        })
        .map(|(_, field_view)| field_view.identifier.clone())
        .collect();

    let list_defaults = !matches!(kind, DataClassKind::Patch);

    DataClassView {
        name: name.to_owned(),
        has_fields: !field_views.is_empty(),
        // `@CratestackBuilder(...)`'s arguments (issue #668 phase 2/3) — see
        // `render_builder_args`'s doc for the shape and
        // `DataClassView::builder_args`'s doc for why these three are the
        // ones that can't be recovered from the emitted Dart source alone.
        builder_args: render_builder_args(
            list_defaults,
            &touch_flag_fields,
            &non_defaulting_list_fields,
        ),
        // Every caller wants a builder, including `build_shared_types_file`
        // — see `DataClassView::emit_builder`'s doc.
        emit_builder: true,
        fields: field_views,
    }
}
