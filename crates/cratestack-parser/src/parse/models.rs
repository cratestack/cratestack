use std::collections::BTreeMap;

use cratestack_core::{Attribute, Field, MixinDecl, Model};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, parse_doc_comment, trimmed_span};
use crate::parse::fields::parse_field;

pub(super) fn parse_model_body(
    lines: &[Line<'_>],
) -> Result<(Vec<Field>, Vec<Attribute>), SchemaError> {
    let mut fields = Vec::new();
    let mut attributes = Vec::new();
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("@@") || line.trimmed.starts_with("@use(") {
            pending_docs.clear();
            attributes.push(Attribute {
                raw: line.trimmed.to_owned(),
                span: trimmed_span(line),
            });
            continue;
        }
        if line.trimmed.starts_with('@') {
            return Err(SchemaError::new(
                format!("unsupported model directive `{}`", line.trimmed),
                line.start..line.start + line.raw.len(),
                line.number,
            ));
        }
        fields.push(parse_field(line, std::mem::take(&mut pending_docs))?);
    }
    Ok((fields, attributes))
}

pub(super) fn expand_model_mixins(
    mixins: &[MixinDecl],
    models: &mut [Model],
) -> Result<(), SchemaError> {
    let mixins_by_name = mixins
        .iter()
        .map(|mixin| (mixin.name.clone(), mixin))
        .collect::<BTreeMap<_, _>>();

    for model in models.iter_mut() {
        let mut expanded_fields = Vec::new();
        let mut field_names = model
            .fields
            .iter()
            .map(|field| field.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut retained_attributes = Vec::new();

        for attribute in &model.attributes {
            if !attribute.raw.starts_with("@use(") {
                retained_attributes.push(attribute.clone());
                continue;
            }

            let mixin_names = parse_model_use_attribute(attribute)?;
            for mixin_name in mixin_names {
                let mixin = mixins_by_name.get(&mixin_name).ok_or_else(|| {
                    SchemaError::new(
                        format!(
                            "model `{}` references unknown mixin `{}` in {}",
                            model.name, mixin_name, attribute.raw
                        ),
                        attribute.span.start..attribute.span.end,
                        attribute.span.line,
                    )
                })?;

                for field in &mixin.fields {
                    if field_names.insert(field.name.clone()) {
                        expanded_fields.push(field.clone());
                    }
                }
            }
        }

        expanded_fields.extend(model.fields.clone());
        model.fields = expanded_fields;
        model.attributes = retained_attributes;
    }

    Ok(())
}

fn parse_model_use_attribute(attribute: &Attribute) -> Result<Vec<String>, SchemaError> {
    let Some(inner) = attribute
        .raw
        .strip_prefix("@use(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(SchemaError::new(
            format!("invalid model use attribute `{}`", attribute.raw),
            attribute.span.start..attribute.span.end,
            attribute.span.line,
        ));
    };
    let names = inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(SchemaError::new(
            format!(
                "model use attribute `{}` must list at least one mixin",
                attribute.raw
            ),
            attribute.span.start..attribute.span.end,
            attribute.span.line,
        ));
    }
    Ok(names)
}
