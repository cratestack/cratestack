//! Relation codegen: `@relation` attribute parsing + per-model relation
//! path emission for the typed query builder, plus the axum-side
//! filter-key/order-key match-arm generators.
//!
//! Submodules:
//! - [`types`] / [`parse`]: parser + shared types (`RelationLink`).
//! - [`query_guard`]: prefix-match guard for `where`-side relation filters.
//! - [`order_arms`]: the model's own top-level sortable field names
//!   (`allowed_sorts`).
//! - [`order_catalog`]: per-model `OrderCatalog` static emission for the
//!   REST `?orderBy=`/`?sort=` runtime resolver — replaces the old
//!   per-path match-arm enumeration (cratestack#256).
//! - [`flat`]: the per-model `RelPath` / `RelToMany` / `Field` emitter.
//!   One arrival type per model, path carried as runtime data — see the
//!   module docs for why this replaced the per-path recursive emitter
//!   (cratestack#252).
//! - [`root`]: per-(model, relation) `Root` entry carrying `as_include()`.
//! - [`include_arm`] / [`include_validation`]: serializer / validator arms
//!   consumed by the per-model axum module.
//! - [`filter_builders`]: shared per-arity append helpers.

mod filter_builders;
mod flat;
mod include_arm;
mod include_validation;
mod order_arms;
mod order_catalog;
mod parse;
mod root;
mod types;

mod query_guard;

pub(crate) use flat::generate_model_path_types;
pub(crate) use include_arm::generate_relation_include_arm;
pub(crate) use include_validation::{
    generate_relation_include_fields_validation_arm, generate_relation_include_path_validation_arm,
};
pub(crate) use order_arms::collect_allowed_sort_keys;
pub(crate) use order_catalog::{generate_model_order_catalog, order_catalog_ident};
pub(crate) use parse::parse_relation_attribute;
pub(crate) use query_guard::generate_relation_query_guard;
pub(crate) use root::generate_relation_root_module;
pub(crate) use types::{RelationLink, relation_link};

#[cfg(test)]
mod tests {
    use cratestack_core::{Attribute, Field, SourceSpan, TypeRef};

    use super::parse::{parse_relation_attribute, split_top_level};

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
                int_args: Vec::new(),
            },
            attributes: vec![Attribute {
                raw: raw.to_owned(),
                span: span(),
            }],
            span: span(),
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
    fn parse_relation_attribute_tolerates_on_delete_and_on_update() {
        // Codegen doesn't act on these — cratestack-migrate does — but
        // it must not reject a relation just because they're present.
        let field = field_with_relation(
            "@relation(fields:[userId], references:[id], onDelete: Cascade, onUpdate: Restrict)",
        );
        let parsed = parse_relation_attribute(&field).expect("relation attribute should parse");
        assert_eq!(parsed.fields, vec!["userId".to_owned()]);
        assert_eq!(parsed.references, vec!["id".to_owned()]);
    }

    #[test]
    fn parse_relation_attribute_tolerates_any_other_unrecognised_key() {
        // Review finding on #261 (the same parser shape, different
        // crate): an unrecognised key used to drop the whole relation
        // instead of just being ignored. `cratestack-parser` is the
        // real vocabulary gatekeeper; this crate only needs
        // `fields`/`references` and shouldn't re-reject a schema that
        // already passed `check` just because its own match doesn't
        // name every key that parser accepts.
        let field =
            field_with_relation("@relation(fields:[userId], references:[id], futureKey: Whatever)");
        let parsed = parse_relation_attribute(&field).expect("relation attribute should parse");
        assert_eq!(parsed.fields, vec!["userId".to_owned()]);
        assert_eq!(parsed.references, vec!["id".to_owned()]);
    }
}
