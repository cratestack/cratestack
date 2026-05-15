//! Resolve a `field` on a model to a (target_model, filter_column,
//! filter_cast, filter_value) tuple suitable for
//! [`crate::data::DataSource::follow`].
//!
//! Two relation shapes are handled in Phase 1b:
//!
//! 1. **Outgoing 1-1 / many-1** — the field carries an explicit
//!    `@relation(fields: [fkCol], references: [pkCol])`. Following
//!    this field reads the target row whose PK equals this row's FK
//!    column.
//!
//! 2. **Inbound 1-many** — the field is a list of another model whose
//!    own field carries the matching `@relation(fields: […])`. The
//!    target rows are the ones where the FK column equals this row's
//!    PK.
//!
//! Many-to-many through a junction table is out of scope for Phase 1b
//! and surfaces as [`DataError::Unsupported`].

use cratestack_core::{Field, Model, Schema, TypeArity};
use cratestack_migrate::column_name;

use super::DataError;
use super::model_info::{ModelSqlInfo, PkCast};

/// Resolved relation shape for [`DataSource::follow`].
#[derive(Debug, Clone)]
pub(crate) struct ResolvedRelation<'a> {
    /// The schema model whose rows we'll project.
    pub target_model: &'a Model,
    /// Column on `target_model` that we'll filter.
    pub filter_column: String,
    /// How the filter parameter should be bound (text vs bigint cast).
    /// Matches the SQL-side cast in the underlying source.
    pub filter_cast: PkCast,
    /// Value to bind for that filter. Either the row's PK (inbound
    /// 1-many) or the row's FK column value (outgoing 1-1 / many-1).
    pub filter_value: FilterValueSource,
    /// `true` when the resolved relation expects exactly one target row
    /// (outgoing 1-1 or many-1). The API endpoint uses this to decide
    /// between returning a single row and a page.
    pub single: bool,
}

/// Where the filter value comes from on the source row. Always the
/// first entry in the field's `@relation(fields: [...])` array.
#[derive(Debug, Clone)]
pub(crate) struct FilterValueSource {
    pub field_name: String,
}

/// Resolve `source_model.field_name` to a relation we can follow.
///
/// CrateStack requires `@relation(fields: [SRC], references: [TGT])`
/// on both sides of a relation, so the resolution is direction-
/// agnostic: the target model is the field's declared type, the
/// source row supplies `fields[0]`, and we filter the target table on
/// `references[0]`. The field's arity decides whether the API returns
/// a single row (Required) or a page (List).
pub(crate) fn resolve_relation<'a>(
    schema: &'a Schema,
    source_model: &Model,
    field_name: &str,
) -> Result<ResolvedRelation<'a>, DataError> {
    let field = source_model
        .fields
        .iter()
        .find(|f| f.name == field_name)
        .ok_or_else(|| DataError::UnknownField {
            model: source_model.name.clone(),
            field: field_name.to_owned(),
        })?;

    let target_model_name = field.ty.name.as_str();
    let target_model = schema
        .models
        .iter()
        .find(|m| m.name == target_model_name)
        .ok_or_else(|| DataError::NotARelation {
            model: source_model.name.clone(),
            field: field_name.to_owned(),
        })?;

    let relation =
        parse_relation_attribute(field).ok_or_else(|| DataError::NotARelation {
            model: source_model.name.clone(),
            field: field_name.to_owned(),
        })?;

    let source_field_name = relation.fields.first().cloned().ok_or(DataError::Unsupported {
        what: "@relation has no `fields:` array (Phase 1b requires exactly one)",
    })?;
    let target_field_name = relation
        .references
        .first()
        .cloned()
        .ok_or(DataError::Unsupported {
            what: "@relation has no `references:` array (Phase 1b requires exactly one)",
        })?;

    // The cast comes from the target column's declared type, since we
    // bind on the target side of the SQL filter.
    let target_field =
        target_model
            .fields
            .iter()
            .find(|f| f.name == target_field_name)
            .ok_or(DataError::Unsupported {
                what: "@relation references a target field that doesn't exist",
            })?;
    let filter_cast = pk_cast_for(&target_field.ty.name).ok_or(DataError::Unsupported {
        what: "relation target column has an unsupported scalar type",
    })?;

    // Validate that the source field exists on this model (catches
    // typos in the schema's `fields:` array early).
    let source_field_exists = source_model.fields.iter().any(|f| f.name == source_field_name);
    if !source_field_exists {
        return Err(DataError::Unsupported {
            what: "@relation `fields:` references a field that doesn't exist on the source model",
        });
    }

    Ok(ResolvedRelation {
        target_model,
        filter_column: column_name(&target_field_name),
        filter_cast,
        filter_value: FilterValueSource {
            field_name: source_field_name,
        },
        single: !matches!(field.ty.arity, TypeArity::List),
    })
}

#[derive(Debug, Default)]
struct ParsedRelation {
    fields: Vec<String>,
    references: Vec<String>,
}

/// Best-effort string parser for `@relation(fields: [a, b], references: [c, d])`.
/// Tolerant of whitespace and missing arrays; returns `None` if the
/// attribute isn't a relation declaration.
fn parse_relation_attribute(field: &Field) -> Option<ParsedRelation> {
    let raw = field
        .attributes
        .iter()
        .find(|a| a.raw.starts_with("@relation"))?
        .raw
        .as_str();
    let mut parsed = ParsedRelation::default();
    if let Some(list) = extract_array(raw, "fields") {
        parsed.fields = list;
    }
    if let Some(list) = extract_array(raw, "references") {
        parsed.references = list;
    }
    Some(parsed)
}

fn extract_array(raw: &str, key: &str) -> Option<Vec<String>> {
    let needle = format!("{key}:");
    let start = raw.find(&needle)? + needle.len();
    let tail = raw[start..].trim_start();
    let tail = tail.strip_prefix('[')?;
    let end = tail.find(']')?;
    Some(
        tail[..end]
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

fn pk_cast_for(scalar: &str) -> Option<PkCast> {
    match scalar {
        "String" | "Uuid" | "Cuid" | "Decimal" => Some(PkCast::Text),
        "Int" => Some(PkCast::BigInt),
        _ => None,
    }
}

/// Helper for endpoints: given a source row (as JSON) and the resolved
/// relation, pull out the value to bind as the filter parameter.
pub(crate) fn extract_filter_value(
    row: &serde_json::Map<String, serde_json::Value>,
    _source_info: &ModelSqlInfo<'_>,
    relation: &ResolvedRelation<'_>,
) -> Result<String, DataError> {
    row.get(&relation.filter_value.field_name)
        .map(super::model_info::json_value_to_cursor)
        .ok_or(DataError::Unsupported {
            what: "row missing the column referenced by @relation(fields: [...])",
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Schema {
        cratestack_parser::parse_schema(text).expect("schema parses")
    }

    const BLOG_SCHEMA: &str = r#"
        model Post {
          id String @id
          title String
          authorId String
          author User @relation(fields: [authorId], references: [id])
        }

        model User {
          id String @id
          name String
          posts Post[] @relation(fields: [id], references: [authorId])
        }
    "#;

    #[test]
    fn outgoing_relation_resolves_to_target_pk_filter() {
        let schema = parse(BLOG_SCHEMA);
        let post = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let resolved = resolve_relation(&schema, post, "author").expect("resolves");
        assert_eq!(resolved.target_model.name, "User");
        assert_eq!(resolved.filter_column, "id");
        assert_eq!(resolved.filter_cast, PkCast::Text);
        assert!(resolved.single);
        assert_eq!(resolved.filter_value.field_name, "authorId");
    }

    #[test]
    fn inbound_one_to_many_resolves_to_fk_column_filter() {
        let schema = parse(BLOG_SCHEMA);
        let user = schema.models.iter().find(|m| m.name == "User").unwrap();
        let resolved = resolve_relation(&schema, user, "posts").expect("resolves");
        assert_eq!(resolved.target_model.name, "Post");
        assert_eq!(resolved.filter_column, "author_id");
        assert_eq!(resolved.filter_cast, PkCast::Text);
        assert!(!resolved.single);
        assert_eq!(resolved.filter_value.field_name, "id");
    }

    #[test]
    fn unknown_field_errors() {
        let schema = parse(BLOG_SCHEMA);
        let post = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let error = resolve_relation(&schema, post, "nope").expect_err("unknown field");
        assert!(matches!(error, DataError::UnknownField { .. }));
    }

    #[test]
    fn non_relation_field_errors() {
        let schema = parse(BLOG_SCHEMA);
        let post = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let error = resolve_relation(&schema, post, "title").expect_err("scalar field");
        assert!(matches!(error, DataError::NotARelation { .. }));
    }

    #[test]
    fn extract_filter_value_reads_field_from_row() {
        let schema = parse(BLOG_SCHEMA);
        let post = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let (_, info) = crate::data::model_info::resolve_model(&schema, "Post").unwrap();
        let resolved = resolve_relation(&schema, post, "author").unwrap();

        let mut row = serde_json::Map::new();
        row.insert("id".to_owned(), serde_json::json!("post-1"));
        row.insert("authorId".to_owned(), serde_json::json!("user-7"));
        row.insert("title".to_owned(), serde_json::json!("Hello"));

        let value = extract_filter_value(&row, &info, &resolved).expect("extracts");
        assert_eq!(value, "user-7");
    }
}
