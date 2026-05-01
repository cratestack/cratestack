use cratestack_core::{Field, Model};

use crate::diagnostics::{SchemaError, span_error};

pub(crate) struct ParsedRelationAttribute {
    pub(crate) fields: Vec<String>,
    pub(crate) references: Vec<String>,
}

pub(crate) fn parse_relation_attribute(raw: &str) -> Result<ParsedRelationAttribute, String> {
    let inner = raw
        .trim()
        .strip_prefix("@relation(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| "invalid @relation attribute syntax".to_owned())?;

    let mut fields = None;
    let mut references = None;
    for entry in split_top_level(inner, ',') {
        let (key, value) = entry
            .split_once(':')
            .ok_or_else(|| format!("invalid @relation entry `{entry}`"))?;
        match key.trim() {
            "fields" => fields = Some(parse_relation_list(value.trim())?),
            "references" => references = Some(parse_relation_list(value.trim())?),
            other => return Err(format!("unsupported @relation key `{other}`")),
        }
    }

    Ok(ParsedRelationAttribute {
        fields: fields.ok_or_else(|| "@relation(...) is missing fields:[...]".to_owned())?,
        references: references
            .ok_or_else(|| "@relation(...) is missing references:[...]".to_owned())?,
    })
}

fn parse_relation_list(value: &str) -> Result<Vec<String>, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("expected relation list syntax like [field], got `{value}`"))?;
    let values = inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err("relation lists must include at least one field".to_owned());
    }
    Ok(values)
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ch if ch == separator && depth == 0 => {
                entries.push(input[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    entries.push(input[start..].trim());
    entries
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .collect()
}

pub(crate) fn validate_relation_scalar_compatibility(
    relation_field: &Field,
    model: &Model,
    local_field: &Field,
    target_field: &Field,
) -> Result<(), SchemaError> {
    if local_field.ty.name != target_field.ty.name {
        return Err(span_error(
            format!(
                "relation field `{}` on model `{}` links incompatible scalar types: local field `{}` is `{}` but referenced field `{}` is `{}`",
                relation_field.name,
                model.name,
                local_field.name,
                local_field.ty.name,
                target_field.name,
                target_field.ty.name,
            ),
            relation_field.span,
        ));
    }
    Ok(())
}
