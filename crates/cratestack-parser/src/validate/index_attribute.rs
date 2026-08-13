//! Semantic checks for the model-level `@@index([...], using: ...,
//! opclass: "...")` attribute (cratestack#156 — pgvector phase 2: ivfflat/
//! hnsw ANN indexes, though the attribute itself is general-purpose, not
//! pgvector-specific). Resolves every listed field against the model's
//! real scalar fields, the same discipline `@@id([...])`/`@@unique([...])`
//! already use (see [`super::composite_attributes`]) — a typo or a
//! relation field is a schema error at `cratestack check` time rather
//! than an index that silently never reaches the database (issue #262's
//! precedent).

use std::collections::BTreeSet;

use cratestack_core::{Attribute, Model, parse_index_attribute};

use crate::diagnostics::{SchemaError, span_error};

use super::composite_attributes::resolve_scalar_field;

/// One `(fields, using)` pair per `@@index([...])` attribute already
/// validated on this model — tracked so two attributes declaring the
/// exact same index (same columns *and* the same access method) are
/// caught rather than silently colliding on the generated index name.
/// Two `@@index` attributes over the *same* fields but *different*
/// `using:` values are legitimate (e.g. a default btree index plus a
/// specialized `ivfflat` one) and must not collide here.
pub(super) type SeenIndexAttributes = Vec<(Vec<String>, Option<String>)>;

pub(super) fn validate_index_attribute(
    model: &Model,
    attribute: &Attribute,
    model_names: &BTreeSet<&str>,
    seen: &mut SeenIndexAttributes,
) -> Result<(), SchemaError> {
    if !attribute.raw.starts_with("@@index(") {
        return Err(span_error(
            format!(
                "model `{}` `@@index` requires a field list: `@@index([field1, field2])`",
                model.name,
            ),
            attribute.span,
        ));
    }

    let parsed = parse_index_attribute(&attribute.raw)
        .map_err(|message| span_error(message, attribute.span))?;

    for field_name in &parsed.fields {
        resolve_scalar_field(model, attribute, model_names, field_name, "@@index([...])")?;
    }

    let key = (parsed.fields.clone(), parsed.using.clone());
    if seen.contains(&key) {
        let using_suffix = parsed
            .using
            .as_deref()
            .map(|using| format!(", using: {using}"))
            .unwrap_or_default();
        return Err(span_error(
            format!(
                "model `{}` declares the same `@@index([{}]{using_suffix})` constraint more than once",
                model.name,
                parsed.fields.join(", "),
            ),
            attribute.span,
        ));
    }
    seen.push(key);

    Ok(())
}
