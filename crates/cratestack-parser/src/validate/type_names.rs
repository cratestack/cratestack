use std::collections::BTreeSet;

use cratestack_core::{Field, Schema, SourceSpan, TypeRef};

use crate::diagnostics::{SchemaError, span_error};

pub(super) const BUILTIN_TYPES: &[&str] = &[
    "String",
    "Cuid",
    "Int",
    "Float",
    "Boolean",
    "DateTime",
    "Decimal",
    "Json",
    "Bytes",
    "Uuid",
    "Page",
    "PageInput",
];

pub(super) fn collect_type_names(schema: &Schema) -> Result<BTreeSet<String>, SchemaError> {
    let mut type_names = BTreeSet::new();
    for builtin in BUILTIN_TYPES {
        type_names.insert((*builtin).to_owned());
    }
    for ty in &schema.types {
        ensure_unique(&mut type_names, &ty.name, ty.span, "duplicate type name")?;
    }
    for enum_decl in &schema.enums {
        ensure_unique(
            &mut type_names,
            &enum_decl.name,
            enum_decl.span,
            "duplicate enum name",
        )?;
    }
    for model in &schema.models {
        ensure_unique(
            &mut type_names,
            &model.name,
            model.span,
            "duplicate model name",
        )?;
    }
    for mixin in &schema.mixins {
        ensure_unique(
            &mut type_names,
            &mixin.name,
            mixin.span,
            "duplicate mixin name",
        )?;
    }
    if let Some(auth) = &schema.auth {
        ensure_unique(
            &mut type_names,
            &auth.name,
            auth.span,
            "duplicate auth type name",
        )?;
    }
    Ok(type_names)
}

pub(super) fn ensure_unique(
    names: &mut BTreeSet<String>,
    name: &str,
    span: SourceSpan,
    message: &str,
) -> Result<(), SchemaError> {
    if !names.insert(name.to_owned()) {
        return Err(span_error(format!("{message} `{name}`"), span));
    }
    Ok(())
}

pub(super) fn validate_type_ref(
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    type_ref: &TypeRef,
    span: SourceSpan,
    allow_page: bool,
    allow_page_input: bool,
) -> Result<(), SchemaError> {
    if type_ref.is_page_input() {
        if !allow_page_input {
            return Err(span_error(
                "built-in `PageInput` is currently only supported as a procedure argument type"
                    .to_owned(),
                span,
            ));
        }
        if type_ref.arity == cratestack_core::TypeArity::List {
            return Err(span_error(
                "built-in `PageInput` cannot be list-valued".to_owned(),
                span,
            ));
        }
        if !type_ref.generic_args.is_empty() {
            return Err(span_error(
                "built-in `PageInput` does not take generic arguments".to_owned(),
                span,
            ));
        }
        return Ok(());
    }

    if type_ref.is_page() {
        if !allow_page {
            return Err(span_error(
                "built-in `Page<T>` is currently only supported as a procedure return type"
                    .to_owned(),
                span,
            ));
        }
        if type_ref.arity != cratestack_core::TypeArity::Required {
            return Err(span_error(
                "built-in `Page<T>` cannot be optional or list-valued".to_owned(),
                span,
            ));
        }
        let Some(item) = type_ref.page_item() else {
            return Err(span_error(
                "built-in `Page<T>` requires exactly one inner type".to_owned(),
                span,
            ));
        };
        if item.is_page() {
            return Err(span_error(
                "nested `Page<Page<T>>` return types are unsupported".to_owned(),
                span,
            ));
        }
        if item.arity != cratestack_core::TypeArity::Required {
            return Err(span_error(
                "built-in `Page<T>` requires a required model or type item".to_owned(),
                span,
            ));
        }
        if !item.generic_args.is_empty() {
            return Err(span_error(
                "built-in `Page<T>` only supports a direct model or type item".to_owned(),
                span,
            ));
        }
        if !page_item_type_names.contains(&item.name) {
            return Err(span_error(
                format!(
                    "built-in `Page<T>` only supports declared model or type items; `{}` is unsupported",
                    item.name
                ),
                span,
            ));
        }
        return Ok(());
    }

    if !type_ref.generic_args.is_empty() {
        return Err(span_error(
            format!("unsupported generic type `{}`", type_ref.name),
            span,
        ));
    }
    if !type_names.contains(&type_ref.name) {
        return Err(span_error(
            format!("unknown type `{}`", type_ref.name),
            span,
        ));
    }
    Ok(())
}

pub(super) fn collect_type_decl_names(schema: &Schema) -> BTreeSet<&str> {
    schema.types.iter().map(|ty| ty.name.as_str()).collect()
}

/// Reject a model field whose type resolves to a `type` declaration.
///
/// `type` blocks are not backed by a database column: the Postgres emitter
/// has a `ColumnType::UserDefined` branch that renders a bare composite
/// type name (e.g. `address`), but nothing in the migrate crate ever emits
/// a matching `CREATE TYPE ... AS (...)` for it — only enums get a
/// type-creating op. A model field typed with a `type` declaration
/// therefore passed `check` before this fix but emitted DDL that fails at
/// `psql` time (`type "address" does not exist`), and the schema macros
/// panicked at expansion regardless (see #230). Reject it here, at the
/// single place a developer already learns about schema problems, rather
/// than downstream in the emitter or the macros.
///
/// This is scoped to a `type` used as a model field's *storage* type only.
/// `type` blocks referencing a `model` (#137,
/// `tests/type_block_model_reference.rs`) and `type` blocks used as
/// procedure args/return types are unaffected — both flow through
/// `validate_type_ref` elsewhere, not through this model-field check.
pub(super) fn reject_type_decl_as_model_field_type(
    type_decl_names: &BTreeSet<&str>,
    model_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    if type_decl_names.contains(field.ty.name.as_str()) {
        return Err(span_error(
            format!(
                "field `{}` on model `{}` cannot use `type {}` as its storage type — `type` \
                 blocks are not backed by a database column (Postgres has no `CREATE TYPE` \
                 emitted for it, and the schema macros cannot encode or decode it); use a \
                 scalar, an `enum`, or a `@relation` to another `model` instead, or inline \
                 `{}`'s fields directly on `{}`",
                field.name, model_name, field.ty.name, field.ty.name, model_name
            ),
            field.span,
        ));
    }
    Ok(())
}
