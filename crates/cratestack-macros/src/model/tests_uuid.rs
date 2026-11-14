//! `Uuid` through the per-model generators (cratestack#1359): the
//! `<Model>Where` filter operands and the model struct accept text or 16
//! bytes under CBOR via `cratestack_core::lenient_uuid`.

use std::collections::BTreeSet;

use super::find_many_where::generate_where_struct;
use super::generate_model_struct_only;
use crate::shared::test_support::render;
use crate::shared::model_name_set;

const FIXTURE: &str = r#"
model Doc {
  id Uuid @id
  ownerId Uuid
  parentId Uuid?
  title String

  @@allow("all", auth() != null)
}
"#;

#[test]
fn where_struct_uses_the_lenient_filter_deserializer_for_uuid_fields_only() {
    let schema = cratestack_parser::parse_schema(FIXTURE).expect("fixture parses");
    let model = &schema.models[0];
    let names = model_name_set(&schema.models);
    let rendered = render(&generate_where_struct(model, &names, &BTreeSet::new()));
    let attr = r#"# [serde (default , deserialize_with = "::cratestack::deserialize_optional_uuid_filter")]"#;
    for field in ["id", "ownerId", "parentId"] {
        assert!(
            rendered.contains(&format!("{attr} pub {field} :")),
            "`{field}` lacks the lenient attribute"
        );
    }
    assert_eq!(rendered.matches("deserialize_optional_uuid_filter").count(), 3);
}

#[test]
fn model_struct_uuid_fields_pick_the_lenient_helpers() {
    let schema = cratestack_parser::parse_schema(FIXTURE).expect("fixture parses");
    let model = &schema.models[0];
    let names = model_name_set(&schema.models);
    let rendered = render(&generate_model_struct_only(model, &names, &BTreeSet::new()));
    assert!(rendered.contains("deserialize_uuid"), "{rendered}");
    assert!(rendered.contains("deserialize_optional_uuid"), "{rendered}");
}
