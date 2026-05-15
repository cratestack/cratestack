//! Schema search.
//!
//! Phase 4 lets the UI fuzz-match models, fields, enums, and types
//! against a query string so a developer with a sprawling schema can
//! jump straight to the thing they're looking for. The match is
//! case-insensitive substring; we don't pull in a scorer because the
//! schema is small and exact substring already covers the workflow
//! ("where is `customer_id` declared?").

use cratestack_core::Schema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub kind: HitKind,
    pub model: Option<String>,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    Model,
    Field,
    Enum,
    Type,
    Mixin,
    Procedure,
}

/// Return every schema element whose name (or, for fields, the
/// "Model.field" path) contains `query` (case-insensitive). Empty
/// query → empty result; the caller should special-case that.
pub fn search(schema: &Schema, query: &str) -> Vec<SearchHit> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let needle = q.to_ascii_lowercase();
    let mut hits = Vec::new();

    for model in &schema.models {
        if model.name.to_ascii_lowercase().contains(&needle) {
            hits.push(SearchHit {
                kind: HitKind::Model,
                model: Some(model.name.clone()),
                name: model.name.clone(),
                detail: format!("{} field(s)", model.fields.len()),
            });
        }
        for field in &model.fields {
            let path = format!("{}.{}", model.name, field.name);
            if field.name.to_ascii_lowercase().contains(&needle)
                || path.to_ascii_lowercase().contains(&needle)
                || field.ty.name.to_ascii_lowercase().contains(&needle)
            {
                hits.push(SearchHit {
                    kind: HitKind::Field,
                    model: Some(model.name.clone()),
                    name: field.name.clone(),
                    detail: format!("{} {}", field.ty.name, arity_label(field.ty.arity)),
                });
            }
        }
    }

    for ty in &schema.types {
        if ty.name.to_ascii_lowercase().contains(&needle) {
            hits.push(SearchHit {
                kind: HitKind::Type,
                model: None,
                name: ty.name.clone(),
                detail: format!("{} field(s)", ty.fields.len()),
            });
        }
    }

    for enum_ in &schema.enums {
        if enum_.name.to_ascii_lowercase().contains(&needle) {
            hits.push(SearchHit {
                kind: HitKind::Enum,
                model: None,
                name: enum_.name.clone(),
                detail: format!("{} variant(s)", enum_.variants.len()),
            });
        }
        for variant in &enum_.variants {
            if variant.name.to_ascii_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    kind: HitKind::Enum,
                    model: Some(enum_.name.clone()),
                    name: variant.name.clone(),
                    detail: format!("variant of {}", enum_.name),
                });
            }
        }
    }

    for mixin in &schema.mixins {
        if mixin.name.to_ascii_lowercase().contains(&needle) {
            hits.push(SearchHit {
                kind: HitKind::Mixin,
                model: None,
                name: mixin.name.clone(),
                detail: format!("{} field(s)", mixin.fields.len()),
            });
        }
    }

    for proc in &schema.procedures {
        if proc.name.to_ascii_lowercase().contains(&needle) {
            hits.push(SearchHit {
                kind: HitKind::Procedure,
                model: None,
                name: proc.name.clone(),
                detail: format!("{:?}", proc.kind),
            });
        }
    }

    hits
}

fn arity_label(arity: cratestack_core::TypeArity) -> &'static str {
    match arity {
        cratestack_core::TypeArity::Required => "required",
        cratestack_core::TypeArity::Optional => "optional",
        cratestack_core::TypeArity::List => "list",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Schema {
        cratestack_parser::parse_schema(text).expect("schema parses")
    }

    #[test]
    fn finds_model_by_partial_name() {
        let schema = parse(
            r#"
                model Customer {
                  id String @id
                }
                model Order {
                  id String @id
                  customerId String
                }
            "#,
        );
        let hits = search(&schema, "cust");
        assert!(
            hits.iter().any(|h| h.kind == HitKind::Model && h.name == "Customer"),
            "{hits:?}"
        );
        assert!(
            hits.iter().any(|h| h.kind == HitKind::Field && h.name == "customerId"),
            "{hits:?}"
        );
    }

    #[test]
    fn empty_query_returns_empty() {
        let schema = parse("model X {\n  id String @id\n}\n");
        assert!(search(&schema, "").is_empty());
        assert!(search(&schema, "   ").is_empty());
    }

    #[test]
    fn matches_field_type() {
        let schema = parse(
            r#"
                model X {
                  id String @id
                  ts DateTime
                }
            "#,
        );
        let hits = search(&schema, "datetime");
        assert!(hits.iter().any(|h| h.name == "ts"), "{hits:?}");
    }
}
