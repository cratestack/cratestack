use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Schema, SourceSpan, TypeRef, parse_emit_attribute};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_helpers::{parse_relation_attribute, validate_relation_scalar_compatibility};

const BUILTIN_TYPES: &[&str] = &[
    "String", "Cuid", "Int", "Float", "Boolean", "DateTime", "Decimal", "Json", "Bytes", "Uuid",
    "Page",
];

pub(crate) fn validate_schema(
    path: &str,
    source: &str,
    schema: &Schema,
) -> Result<(), SchemaError> {
    let mut type_names = BTreeSet::new();
    for builtin in BUILTIN_TYPES {
        type_names.insert((*builtin).to_owned());
    }
    for ty in &schema.types {
        ensure_unique(
            &mut type_names,
            &ty.name,
            ty.span,
            "duplicate type name",
            source,
            path,
        )?;
    }
    for enum_decl in &schema.enums {
        ensure_unique(
            &mut type_names,
            &enum_decl.name,
            enum_decl.span,
            "duplicate enum name",
            source,
            path,
        )?;
    }
    for model in &schema.models {
        ensure_unique(
            &mut type_names,
            &model.name,
            model.span,
            "duplicate model name",
            source,
            path,
        )?;
    }
    for mixin in &schema.mixins {
        ensure_unique(
            &mut type_names,
            &mixin.name,
            mixin.span,
            "duplicate mixin name",
            source,
            path,
        )?;
    }
    if let Some(auth) = &schema.auth {
        ensure_unique(
            &mut type_names,
            &auth.name,
            auth.span,
            "duplicate auth type name",
            source,
            path,
        )?;
    }

    let mut procedure_names = BTreeSet::new();
    for procedure in &schema.procedures {
        if !procedure_names.insert(procedure.name.clone()) {
            return Err(span_error(
                format!("duplicate procedure name `{}`", procedure.name),
                procedure.span,
            ));
        }
    }

    if let Some(datasource) = &schema.datasource {
        let provider = datasource
            .entries
            .iter()
            .find(|entry| entry.key == "provider")
            .map(|entry| entry.value.trim_matches('"'));

        if let Some(provider) = provider
            && provider != "postgresql"
        {
            return Err(span_error(
                format!("unsupported datasource provider `{provider}`; expected `postgresql`"),
                datasource.span,
            ));
        }
    }

    let model_names = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let page_item_type_names = schema
        .models
        .iter()
        .map(|model| model.name.clone())
        .chain(schema.types.iter().map(|ty| ty.name.clone()))
        .collect::<BTreeSet<_>>();

    for model in &schema.models {
        let mut fields = BTreeMap::new();
        let mut has_primary_key = false;
        let mut saw_emit_attribute = false;
        let mut saw_paged_attribute = false;
        for field in &model.fields {
            if fields.insert(field.name.clone(), field.span).is_some() {
                return Err(span_error(
                    format!("duplicate field `{}` on model `{}`", field.name, model.name),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@id"))
            {
                has_primary_key = true;
            }
            validate_custom_field_attribute(
                field,
                "model",
                &model.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
            validate_validator_attributes(&model.name, field)?;
            validate_field_policy_attributes(&model.name, field)?;

            let relation_attribute = field
                .attributes
                .iter()
                .find(|attribute| attribute.raw.starts_with("@relation("));
            if model_names.contains(field.ty.name.as_str()) {
                let relation_attribute = relation_attribute.ok_or_else(|| {
                    span_error(
                        format!(
                            "relation field `{}` on model `{}` must declare @relation(fields:[...],references:[...])",
                            field.name, model.name,
                        ),
                        field.span,
                    )
                })?;
                let relation = parse_relation_attribute(&relation_attribute.raw)
                    .map_err(|message| span_error(message, field.span))?;
                if relation.fields.len() != 1 || relation.references.len() != 1 {
                    return Err(span_error(
                        format!(
                            "relation field `{}` on model `{}` must declare exactly one local field and one reference in this slice",
                            field.name, model.name,
                        ),
                        field.span,
                    ));
                }

                let local_field = model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == relation.fields[0])
                    .ok_or_else(|| {
                        span_error(
                            format!(
                                "relation field `{}` on model `{}` references unknown local field `{}`",
                                field.name, model.name, relation.fields[0],
                            ),
                            field.span,
                        )
                    })?;
                if model_names.contains(local_field.ty.name.as_str()) {
                    return Err(span_error(
                        format!(
                            "relation field `{}` on model `{}` must use a scalar local field, found relation field `{}`",
                            field.name, model.name, local_field.name,
                        ),
                        field.span,
                    ));
                }

                let target_model = schema
                    .models
                    .iter()
                    .find(|candidate| candidate.name == field.ty.name)
                    .ok_or_else(|| {
                        span_error(
                            format!(
                                "relation field `{}` on model `{}` references unknown target model `{}`",
                                field.name, model.name, field.ty.name,
                            ),
                            field.span,
                        )
                    })?;
                let target_field = target_model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == relation.references[0])
                    .ok_or_else(|| {
                        span_error(
                            format!(
                                "relation field `{}` on model `{}` references unknown target field `{}` on `{}`",
                                field.name, model.name, relation.references[0], target_model.name,
                            ),
                            field.span,
                        )
                    })?;
                if model_names.contains(target_field.ty.name.as_str()) {
                    return Err(span_error(
                        format!(
                            "relation field `{}` on model `{}` must reference a scalar target field, found relation field `{}`",
                            field.name, model.name, target_field.name,
                        ),
                        field.span,
                    ));
                }
                validate_relation_scalar_compatibility(field, model, local_field, target_field)?;
            } else if relation_attribute.is_some() {
                return Err(span_error(
                    format!(
                        "scalar field `{}` on model `{}` cannot declare @relation(...)",
                        field.name, model.name,
                    ),
                    field.span,
                ));
            }
        }

        for attribute in &model.attributes {
            if attribute.raw.starts_with("@@emit(") {
                if saw_emit_attribute {
                    return Err(span_error(
                        format!(
                            "model `{}` must not declare more than one @@emit(...) attribute",
                            model.name
                        ),
                        attribute.span,
                    ));
                }
                parse_emit_attribute(&attribute.raw)
                    .map_err(|message| span_error(message, attribute.span))?;
                saw_emit_attribute = true;
            } else if attribute.raw.starts_with("@@paged") {
                if attribute.raw != "@@paged" {
                    return Err(span_error(
                        format!(
                            "model `{}` uses unsupported paging directive `{}`; use bare `@@paged` in this slice",
                            model.name, attribute.raw,
                        ),
                        attribute.span,
                    ));
                }
                if saw_paged_attribute {
                    return Err(span_error(
                        format!(
                            "model `{}` must not declare more than one @@paged attribute",
                            model.name
                        ),
                        attribute.span,
                    ));
                }
                saw_paged_attribute = true;
            } else if attribute.raw == "@@audit" {
                // recognised; no further validation needed at parse time
            } else if attribute.raw.starts_with("@@audit(") {
                return Err(span_error(
                    format!(
                        "model `{}` `@@audit` does not take arguments; use bare `@@audit`",
                        model.name,
                    ),
                    attribute.span,
                ));
            } else if attribute.raw == "@@soft_delete" {
                // recognised; descriptor wiring lives in the macro
            } else if attribute.raw.starts_with("@@soft_delete(") {
                return Err(span_error(
                    format!(
                        "model `{}` `@@soft_delete` does not take arguments",
                        model.name,
                    ),
                    attribute.span,
                ));
            } else if attribute.raw.starts_with("@@retain(") {
                let inner = attribute
                    .raw
                    .strip_prefix("@@retain(")
                    .and_then(|s| s.strip_suffix(')'))
                    .ok_or_else(|| {
                        span_error(
                            format!("model `{}` `@@retain` is malformed", model.name),
                            attribute.span,
                        )
                    })?
                    .trim();
                let days_str = inner.strip_prefix("days:").map(str::trim).ok_or_else(|| {
                    span_error(
                        format!("model `{}` `@@retain` requires `days: N`", model.name,),
                        attribute.span,
                    )
                })?;
                days_str.parse::<u32>().map_err(|_| {
                    span_error(
                        format!(
                            "model `{}` `@@retain(days: ...)` must be a non-negative integer",
                            model.name,
                        ),
                        attribute.span,
                    )
                })?;
            }
        }

        if !has_primary_key {
            return Err(span_error(
                format!("model `{}` is missing an @id field", model.name),
                model.span,
            ));
        }

        let version_fields: Vec<&cratestack_core::Field> = model
            .fields
            .iter()
            .filter(|field| field.attributes.iter().any(|a| a.raw == "@version"))
            .collect();
        if version_fields.len() > 1 {
            return Err(span_error(
                format!(
                    "model `{}` declares more than one @version field",
                    model.name,
                ),
                version_fields[1].span,
            ));
        }
        if let Some(version) = version_fields.first() {
            if version.ty.name != "Int"
                || !matches!(version.ty.arity, cratestack_core::TypeArity::Required)
            {
                return Err(span_error(
                    format!(
                        "@version field `{}.{}` must be a required `Int`",
                        model.name, version.name,
                    ),
                    version.span,
                ));
            }
            if version
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@id"))
            {
                return Err(span_error(
                    format!(
                        "@version field `{}.{}` must not also be the primary key",
                        model.name, version.name,
                    ),
                    version.span,
                ));
            }
        }
    }

    for mixin in &schema.mixins {
        let mut fields = BTreeMap::new();
        for field in &mixin.fields {
            if fields.insert(field.name.clone(), field.span).is_some() {
                return Err(span_error(
                    format!("duplicate field `{}` on mixin `{}`", field.name, mixin.name),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@id"))
            {
                return Err(span_error(
                    format!(
                        "field `{}` on mixin `{}` cannot declare @id",
                        field.name, mixin.name
                    ),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@@"))
            {
                return Err(span_error(
                    format!(
                        "field `{}` on mixin `{}` cannot declare model-level attributes",
                        field.name, mixin.name
                    ),
                    field.span,
                ));
            }
            validate_custom_field_attribute(
                field,
                "mixin",
                &mixin.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }

    for ty in &schema.types {
        let mut fields = BTreeSet::new();
        for field in &ty.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!("duplicate field `{}` on type `{}`", field.name, ty.name),
                    field.span,
                ));
            }
            validate_custom_field_attribute(field, "type", &ty.name, CustomFieldSupport::TypeOnly)?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }

    for enum_decl in &schema.enums {
        let mut variants = BTreeSet::new();
        for variant in &enum_decl.variants {
            if !variants.insert(variant.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate variant `{}` on enum `{}`",
                        variant.name, enum_decl.name
                    ),
                    variant.span,
                ));
            }
        }
    }

    if let Some(auth) = &schema.auth {
        let mut fields = BTreeSet::new();
        for field in &auth.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate field `{}` on auth block `{}`",
                        field.name, auth.name
                    ),
                    field.span,
                ));
            }
            validate_custom_field_attribute(
                field,
                "auth block",
                &auth.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }

    for procedure in &schema.procedures {
        for arg in &procedure.args {
            validate_type_ref(
                &type_names,
                &page_item_type_names,
                &arg.ty,
                procedure.span,
                false,
            )?;
        }
        validate_type_ref(
            &type_names,
            &page_item_type_names,
            &procedure.return_type,
            procedure.span,
            true,
        )?;
        validate_procedure_isolation_attribute(procedure)?;
        validate_procedure_api_version_attribute(procedure)?;
        validate_procedure_deprecated_attribute(procedure)?;
    }

    let _ = (path, source);
    Ok(())
}

#[derive(Clone, Copy)]
enum CustomFieldSupport {
    Rejected,
    TypeOnly,
}

fn validate_custom_field_attribute(
    field: &Field,
    owner_kind: &str,
    owner_name: &str,
    support: CustomFieldSupport,
) -> Result<(), SchemaError> {
    let mut custom_count = 0usize;
    for attribute in &field.attributes {
        if !attribute.raw.starts_with("@custom") {
            continue;
        }
        custom_count += 1;
        if attribute.raw != "@custom" {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses unsupported custom field directive `{}`; use bare `@custom` in this slice",
                    field.name, owner_kind, owner_name, attribute.raw,
                ),
                field.span,
            ));
        }
        if matches!(support, CustomFieldSupport::Rejected) {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` cannot use `@custom`; resolver-backed custom fields are currently only supported on `type` declarations",
                    field.name, owner_kind, owner_name,
                ),
                field.span,
            ));
        }
    }

    if custom_count > 1 {
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` declares `@custom` more than once",
                field.name, owner_kind, owner_name,
            ),
            field.span,
        ));
    }

    Ok(())
}

fn ensure_unique(
    names: &mut BTreeSet<String>,
    name: &str,
    span: SourceSpan,
    message: &str,
    _source: &str,
    _path: &str,
) -> Result<(), SchemaError> {
    if !names.insert(name.to_owned()) {
        return Err(span_error(format!("{message} `{name}`"), span));
    }
    Ok(())
}

fn validate_type_ref(
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    type_ref: &TypeRef,
    span: SourceSpan,
    allow_page: bool,
) -> Result<(), SchemaError> {
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

/// Recognise the validation attribute family (`@length`, `@range`, `@regex`,
/// `@email`, `@uri`, `@iso4217`) and reject combinations that don't match the
/// field's scalar type. This is parse-time only — runtime enforcement happens
/// in generated `validate` impls on Create/Update inputs.
fn validate_validator_attributes(
    model_name: &str,
    field: &cratestack_core::Field,
) -> Result<(), SchemaError> {
    let scalar = field.ty.name.as_str();
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        let (name, has_args) = if let Some(open) = raw.find('(') {
            (&raw[1..open], true)
        } else {
            (&raw[1..], false)
        };
        match name {
            "length" => {
                if !has_args {
                    return Err(span_error(
                        format!(
                            "field `{}.{}` @length requires arguments like @length(min: 1, max: 200)",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                if scalar != "String" && scalar != "Bytes" {
                    return Err(span_error(
                        format!(
                            "@length on `{}.{}` is only valid on String or Bytes fields",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                parse_length_args(raw).map_err(|message| {
                    span_error(
                        format!("field `{}.{}`: {message}", model_name, field.name,),
                        field.span,
                    )
                })?;
            }
            "range" => {
                if !has_args {
                    return Err(span_error(
                        format!(
                            "field `{}.{}` @range requires arguments like @range(min: 0, max: 100)",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                if scalar != "Int" && scalar != "Decimal" {
                    return Err(span_error(
                        format!(
                            "@range on `{}.{}` is only valid on Int or Decimal fields",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                parse_range_args(raw).map_err(|message| {
                    span_error(
                        format!("field `{}.{}`: {message}", model_name, field.name,),
                        field.span,
                    )
                })?;
            }
            "regex" => {
                if !has_args {
                    return Err(span_error(
                        format!(
                            "field `{}.{}` @regex requires a string argument",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                if scalar != "String" {
                    return Err(span_error(
                        format!(
                            "@regex on `{}.{}` is only valid on String fields",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                parse_regex_arg(raw).map_err(|message| {
                    span_error(
                        format!("field `{}.{}`: {message}", model_name, field.name,),
                        field.span,
                    )
                })?;
            }
            "email" | "uri" | "iso4217" => {
                if has_args {
                    return Err(span_error(
                        format!(
                            "field `{}.{}` @{name} does not take arguments",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
                if scalar != "String" {
                    return Err(span_error(
                        format!(
                            "@{name} on `{}.{}` is only valid on String fields",
                            model_name, field.name,
                        ),
                        field.span,
                    ));
                }
            }
            _ => {} // unknown attribute; left to other validators
        }
    }
    Ok(())
}

/// Parse `@length(min: N, max: N)` into `(min, max)` with both bounds optional.
pub(crate) fn parse_length_args(raw: &str) -> Result<(Option<u32>, Option<u32>), String> {
    let inner = strip_attribute_parens(raw, "length")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: u32 = value
            .parse()
            .map_err(|_| format!("@length expects non-negative integer, got `{value}`"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            other => return Err(format!("@length: unknown argument `{other}`")),
        }
    }
    if let (Some(lo), Some(hi)) = (min, max) {
        if lo > hi {
            return Err(format!("@length: min ({lo}) must be <= max ({hi})"));
        }
    }
    Ok((min, max))
}

/// Parse `@range(min: N, max: N)` into `(min, max)` with both bounds optional
/// and signed.
pub(crate) fn parse_range_args(raw: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let inner = strip_attribute_parens(raw, "range")?;
    let (mut min, mut max) = (None, None);
    for part in split_kv_args(&inner) {
        let (key, value) = split_kv(&part)?;
        let parsed: i64 = value
            .parse()
            .map_err(|_| format!("@range expects integer, got `{value}`"))?;
        match key.as_str() {
            "min" => min = Some(parsed),
            "max" => max = Some(parsed),
            other => return Err(format!("@range: unknown argument `{other}`")),
        }
    }
    if let (Some(lo), Some(hi)) = (min, max) {
        if lo > hi {
            return Err(format!("@range: min ({lo}) must be <= max ({hi})"));
        }
    }
    Ok((min, max))
}

/// Parse `@regex("pattern")` into the pattern string. Validates the regex
/// compiles so we fail at schema-load time rather than first request.
pub(crate) fn parse_regex_arg(raw: &str) -> Result<String, String> {
    let inner = strip_attribute_parens(raw, "regex")?;
    let trimmed = inner.trim();
    let stripped = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| "@regex argument must be a quoted string literal".to_owned())?;
    regex::Regex::new(stripped).map_err(|e| format!("@regex pattern is not a valid regex: {e}"))?;
    Ok(stripped.to_owned())
}

fn strip_attribute_parens(raw: &str, name: &str) -> Result<String, String> {
    let prefix = format!("@{name}(");
    let trimmed = raw
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("@{name} attribute is malformed"))?;
    let inner = trimmed
        .strip_suffix(')')
        .ok_or_else(|| format!("@{name} attribute is missing closing paren"))?;
    Ok(inner.to_owned())
}

fn split_kv_args(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_kv(part: &str) -> Result<(String, String), String> {
    let (key, value) = part
        .split_once(':')
        .ok_or_else(|| format!("expected `key: value`, got `{part}`"))?;
    Ok((key.trim().to_owned(), value.trim().to_owned()))
}

/// Parse and validate the `@isolation("...")` procedure attribute. At most
/// one is permitted per procedure; the level string must be one of the
/// values [`cratestack_core::TransactionIsolation::parse`] accepts.
fn validate_procedure_isolation_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@isolation"))
        .collect();
    if matches.is_empty() {
        return Ok(());
    }
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @isolation attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let attr = matches[0];
    let inner = attr
        .raw
        .strip_prefix("@isolation(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @isolation requires a quoted level argument like @isolation(\"serializable\")",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    let level = inner
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @isolation argument must be a quoted string",
                    procedure.name,
                ),
                attr.span,
            )
        })?;
    cratestack_core::TransactionIsolation::parse(level).map_err(|error| {
        span_error(
            format!(
                "procedure `{}` @isolation: {}",
                procedure.name,
                error.public_message(),
            ),
            attr.span,
        )
    })?;
    Ok(())
}

/// Validate `@api_version("v1")` on procedures. The value is opaque to the
/// parser — banks pick their own scheme (semver, calver, mvX). We only
/// enforce non-empty and ASCII-printable so it can safely flow into URL
/// route segments.
fn validate_procedure_api_version_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@api_version"))
        .collect();
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @api_version attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let Some(attr) = matches.first() else {
        return Ok(());
    };
    let inner = attr
        .raw
        .strip_prefix("@api_version(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @api_version requires a quoted version argument",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    let stripped = inner
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @api_version argument must be a quoted string",
                    procedure.name,
                ),
                attr.span,
            )
        })?;
    if stripped.is_empty() {
        return Err(span_error(
            format!(
                "procedure `{}` @api_version must not be empty",
                procedure.name,
            ),
            attr.span,
        ));
    }
    if !stripped
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(span_error(
            format!(
                "procedure `{}` @api_version must contain only alphanumeric, '.', '-', or '_' characters",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}

/// Validate `@deprecated("use foo v2")` on procedures. Message is optional;
/// when present, the macro emits a `Deprecation: true` and `X-Deprecation`
/// header carrying the rationale.
fn validate_procedure_deprecated_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw == "@deprecated" || a.raw.starts_with("@deprecated("))
        .collect();
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @deprecated attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let Some(attr) = matches.first() else {
        return Ok(());
    };
    if attr.raw == "@deprecated" {
        return Ok(());
    }
    let inner = attr
        .raw
        .strip_prefix("@deprecated(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @deprecated must be either bare or `@deprecated(\"message\")`",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    if !inner.starts_with('"') || !inner.ends_with('"') {
        return Err(span_error(
            format!(
                "procedure `{}` @deprecated argument must be a quoted string",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}

/// Reject `@readonly` / `@server_only` declared on the primary-key field —
/// PKs are server-controlled anyway and the combination is a likely typo.
fn validate_field_policy_attributes(
    model_name: &str,
    field: &cratestack_core::Field,
) -> Result<(), SchemaError> {
    let is_id = field.attributes.iter().any(|a| a.raw.starts_with("@id"));
    let has_readonly = field.attributes.iter().any(|a| a.raw == "@readonly");
    let has_server_only = field.attributes.iter().any(|a| a.raw == "@server_only");

    if is_id && (has_readonly || has_server_only) {
        let attr = if has_readonly {
            "@readonly"
        } else {
            "@server_only"
        };
        return Err(span_error(
            format!(
                "field `{}.{}` is the primary key and must not declare {attr}",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    if has_readonly && has_server_only {
        return Err(span_error(
            format!(
                "field `{}.{}` declares both @readonly and @server_only; use @server_only alone",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    Ok(())
}
