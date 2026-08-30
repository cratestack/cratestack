//! Exercises [`super::computed_bearing_names`] and
//! [`super::procedure_output_composition`] against real parsed schemas
//! (`cratestack_parser::parse_schema`, the same entry point
//! `include_server_schema!` itself uses) rather than hand-built
//! `Schema` values — a hand-built `Schema` could silently drift from
//! what the parser actually produces (span values, attribute `raw`
//! spelling, ...), which would make these tests prove nothing about the
//! real macro-expansion path.

use super::*;

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("fixture schema should parse")
}

#[test]
fn bearing_includes_a_type_with_its_own_computed_field() {
    let schema = schema("type Image {\n  storageKey String\n  thumbnailUrl String @computed\n}\n");
    let bearing = computed_bearing_names(&schema);
    assert!(bearing.contains("Image"));
}

#[test]
fn bearing_propagates_through_nested_type_fields() {
    let schema = schema(
        "type Image {\n  storageKey String\n  thumbnailUrl String @computed\n}\n\
         type Card {\n  cover Image\n}\n",
    );
    let bearing = computed_bearing_names(&schema);
    assert!(bearing.contains("Image"));
    assert!(
        bearing.contains("Card"),
        "Card nests a bearing type through `cover`"
    );
}

#[test]
fn bearing_propagates_from_a_computed_model_through_a_nesting_type() {
    let schema = schema(
        "model Photo {\n  id Int @id\n  storageKey String\n  proxyUrl String @computed\n}\n\
         type Gallery {\n  cover Photo\n}\n",
    );
    let bearing = computed_bearing_names(&schema);
    assert!(
        bearing.contains("Photo"),
        "a model with its own @computed field is bearing"
    );
    assert!(
        bearing.contains("Gallery"),
        "a type nesting a bearing model directly (not a relation) propagates too"
    );
}

#[test]
fn bearing_excludes_types_and_models_with_no_computed_reach() {
    let schema = schema(
        "type Plain {\n  label String\n}\n\
         model Widget {\n  id Int @id\n  label String\n}\n",
    );
    let bearing = computed_bearing_names(&schema);
    assert!(bearing.is_empty());
}

#[test]
fn compose_fn_ident_snake_cases_the_owner_name() {
    assert_eq!(compose_fn_ident("Image").to_string(), "compose_image_value");
    assert_eq!(
        compose_fn_ident("ProxyParams").to_string(),
        "compose_proxy_params_value"
    );
}

fn required_ref(name: &str) -> cratestack_core::TypeRef {
    cratestack_core::TypeRef {
        name: name.to_owned(),
        name_span: cratestack_core::SourceSpan {
            start: 0,
            end: 0,
            line: 0,
        },
        arity: TypeArity::Required,
        generic_args: Vec::new(),
        int_args: Vec::new(),
        ident_args: Vec::new(),
    }
}

#[test]
fn composition_is_none_for_a_non_bearing_required_return() {
    let bearing = BTreeSet::new();
    assert!(procedure_output_composition(&required_ref("Post"), &bearing).is_none());
}

#[test]
fn composition_is_unary_for_a_bearing_required_return() {
    let mut bearing = BTreeSet::new();
    bearing.insert("Image".to_owned());
    match procedure_output_composition(&required_ref("Image"), &bearing) {
        Some(ProcedureOutputComposition::Unary { owner, optional }) => {
            assert_eq!(owner, "Image");
            assert!(!optional);
        }
        _ => panic!("expected a Unary composition"),
    }
}

#[test]
fn composition_is_unary_optional_for_a_bearing_optional_return() {
    let mut bearing = BTreeSet::new();
    bearing.insert("Image".to_owned());
    let mut ty = required_ref("Image");
    ty.arity = TypeArity::Optional;
    match procedure_output_composition(&ty, &bearing) {
        Some(ProcedureOutputComposition::Unary { owner, optional }) => {
            assert_eq!(owner, "Image");
            assert!(optional);
        }
        _ => panic!("expected a Unary composition"),
    }
}

#[test]
fn composition_is_list_for_a_bearing_bare_list_return() {
    let mut bearing = BTreeSet::new();
    bearing.insert("Image".to_owned());
    let mut ty = required_ref("Image");
    ty.arity = TypeArity::List;
    match procedure_output_composition(&ty, &bearing) {
        Some(ProcedureOutputComposition::List { owner }) => assert_eq!(owner, "Image"),
        _ => panic!("expected a List composition"),
    }
}

#[test]
fn composition_is_page_for_a_bearing_page_item() {
    let mut bearing = BTreeSet::new();
    bearing.insert("Image".to_owned());
    let mut ty = required_ref("Page");
    ty.generic_args = vec![required_ref("Image")];
    match procedure_output_composition(&ty, &bearing) {
        Some(ProcedureOutputComposition::Page { owner }) => assert_eq!(owner, "Image"),
        _ => panic!("expected a Page composition"),
    }
}

#[test]
fn composition_is_none_for_a_non_bearing_page_item() {
    let bearing = BTreeSet::new();
    let mut ty = required_ref("Page");
    ty.generic_args = vec![required_ref("Post")];
    assert!(procedure_output_composition(&ty, &bearing).is_none());
}
