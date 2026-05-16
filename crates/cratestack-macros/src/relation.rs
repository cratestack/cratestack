//! Relation codegen: `@relation` attribute parsing + per-relation
//! filter / order / include module emission for the typed query
//! builder, plus the axum-side filter-key/order-key match-arm
//! generators.
//!
//! Split into focused submodules:
//! - [`types`] / [`parse`]: parser + shared types (`RelationLink`,
//!   `RelationPathSegment`, `ParsedRelationAttribute`).
//! - [`query_guard`]: prefix-match guard for `where`-side relation
//!   filters.
//! - [`order_arms`] / [`order_targets`]: orderBy arm + SQL fragment
//!   computation through to-one relation chains.
//! - [`order_module`] / [`order_recursive`]: top-level relation
//!   `pub mod { ... }` emission + the recursive walker that fills it.
//! - [`quantifier`] / [`recursive_entries`]: to-many `some`/`every`/
//!   `none` containers + the entry builder shared with the recursive
//!   walker.
//! - [`include_arm`] / [`include_validation`]: serializer / validator
//!   arms consumed by the per-model axum module.
//! - [`scalar_builder`] / [`filter_fns`] / [`path_method`] / [`wrap`]:
//!   per-scalar leaf emitters + the filter-expr wrapper.
//! - [`filter_builders`]: shared per-arity append helpers (was a
//!   sibling module in the original layout).

mod filter_builders;
mod filter_fns;
mod include_arm;
mod include_validation;
mod order_arms;
mod order_module;
mod order_recursive;
mod order_targets;
mod parse;
mod path_method;
mod quantifier;
mod recursive_entries;
mod scalar_builder;
mod types;
mod wrap;

mod query_guard;

pub(crate) use include_arm::generate_relation_include_arm;
pub(crate) use include_validation::{
    generate_relation_include_fields_validation_arm, generate_relation_include_path_validation_arm,
};
pub(crate) use order_arms::{collect_allowed_sort_keys, generate_relation_order_by_arms};
pub(crate) use order_module::generate_relation_order_module;
pub(crate) use parse::parse_relation_attribute;
pub(crate) use query_guard::generate_relation_query_guard;
pub(crate) use types::{RelationLink, relation_link};

#[cfg(test)]
mod tests {
    use cratestack_core::{Attribute, Field, SourceSpan, TypeRef};

    use super::parse::{parse_relation_attribute, split_top_level};
    use super::order_recursive::wrappers_allow_ordering;
    use super::types::{RelationFilterWrapperKind, RelationLink, RelationPathSegment};

    fn span() -> SourceSpan {
        SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        }
    }

    fn field_with_relation(raw: &str) -> Field {
        Field {
            docs: Vec::new(),
            name: "author".to_owned(),
            name_span: span(),
            ty: TypeRef {
                name: "User".to_owned(),
                name_span: span(),
                arity: cratestack_core::TypeArity::Required,
                generic_args: Vec::new(),
            },
            attributes: vec![Attribute {
                raw: raw.to_owned(),
                span: span(),
            }],
            span: span(),
        }
    }

    fn segment(kind: RelationFilterWrapperKind) -> RelationPathSegment {
        RelationPathSegment {
            link: RelationLink {
                parent_table: "posts".to_owned(),
                parent_column: "author_id".to_owned(),
                related_table: "users".to_owned(),
                related_column: "id".to_owned(),
                is_to_many: false,
            },
            kind,
        }
    }

    #[test]
    fn split_top_level_ignores_nested_brackets() {
        let items = split_top_level("fields:[userId], references:[id], map:[a,b(c,d)]", ',');
        assert_eq!(
            items,
            vec!["fields:[userId]", "references:[id]", "map:[a,b(c,d)]"]
        );
    }

    #[test]
    fn parse_relation_attribute_extracts_fields_and_references() {
        let field = field_with_relation("@relation(fields:[userId], references:[id])");
        let parsed = parse_relation_attribute(&field).expect("relation attribute should parse");
        assert_eq!(parsed.fields, vec!["userId".to_owned()]);
        assert_eq!(parsed.references, vec!["id".to_owned()]);
    }

    #[test]
    fn parse_relation_attribute_rejects_unknown_keys() {
        let field = field_with_relation("@relation(fields:[userId], ref:[id])");
        assert!(parse_relation_attribute(&field).is_none());
    }

    #[test]
    fn wrappers_allow_ordering_only_for_to_one_paths() {
        assert!(wrappers_allow_ordering(&[segment(
            RelationFilterWrapperKind::ToOne
        )]));
        assert!(!wrappers_allow_ordering(&[segment(
            RelationFilterWrapperKind::Some
        )]));
    }
}
