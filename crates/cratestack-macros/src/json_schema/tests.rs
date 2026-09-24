//! The generator's refusals. Each type with no faithful mapping must come
//! back as a typed error naming the type and where it was reached, never
//! as a schema. Shape tests for what *is* mapped are in `tests_shapes.rs`;
//! agreement with serde is tested in the facade round-trip suites.

use cratestack_core::Schema;

use super::{JsonSchemaError, procedure_input_schema, procedure_output_schema};
use crate::shared::decimal_backend::DecimalBackend;

pub(super) fn parse(source: &str) -> Schema {
    cratestack_parser::parse_schema(source).expect("fixture schema should parse")
}

fn input_error(source: &str) -> JsonSchemaError {
    let schema = parse(source);
    procedure_input_schema(
        &schema,
        &schema.procedures[0],
        Some(DecimalBackend::RustDecimal),
    )
    .expect_err("input schema should be refused")
}

fn refused_type(error: &JsonSchemaError) -> &str {
    match error {
        JsonSchemaError::NoFaithfulMapping { type_name, .. } => type_name,
        other => panic!("expected NoFaithfulMapping, got {other:?}"),
    }
}

#[test]
fn json_argument_is_refused_by_name() {
    let error = input_error("procedure stash(payload: Json): Boolean");
    assert_eq!(refused_type(&error), "Json");
    let message = error.to_string();
    assert!(message.starts_with("`Json` has no faithful JSON Schema mapping"));
    assert!(message.ends_with("(at procedure `stash` → argument `payload`)"));
}

#[test]
fn json_nested_in_a_type_names_the_path() {
    let error = input_error(
        "type Holder {\n  label String\n  payload Json?\n}\n\
         type Outer {\n  holders Holder[]\n}\n\
         procedure put(args: Outer): Boolean",
    );
    assert_eq!(refused_type(&error), "Json");
    assert!(error.to_string().ends_with(
        "(at procedure `put` → argument `args` → field `Outer.holders` → field `Holder.payload`)"
    ));
}

#[test]
fn json_return_type_is_refused_when_the_output_is_an_object() {
    let schema = parse("type Box {\n  data Json\n}\nprocedure get(): Box");
    let error = procedure_output_schema(&schema, &schema.procedures[0], None)
        .expect_err("output schema should be refused");
    assert_eq!(refused_type(&error), "Json");
    assert!(error.to_string().contains("procedure `get` → return type"));
}

#[test]
fn find_many_argument_is_refused() {
    let error =
        input_error("model Post {\n  id Int @id\n}\nprocedure search(q: FindMany<Post>): Post[]");
    assert_eq!(refused_type(&error), "FindMany");
}

#[test]
fn vector_field_is_refused() {
    let error = input_error(
        "extension pgvector {\n}\ntype Probe {\n  embedding Vector(3)\n}\n\
         procedure near(args: Probe): Boolean",
    );
    assert_eq!(refused_type(&error), "Vector");
}

#[test]
fn spatial_fields_are_refused() {
    for ty in ["Geography(Point, 4326)", "Geometry(Point, 4326)"] {
        let error = input_error(&format!(
            "extension postgis {{\n}}\ntype Probe {{\n  area {ty}\n}}\n\
             procedure near(args: Probe): Boolean"
        ));
        let expected = &ty[..ty.find('(').unwrap()];
        assert_eq!(refused_type(&error), expected);
    }
}

#[test]
fn computed_bearing_output_is_refused() {
    // Only the output side can be tested: the parser already rejects a
    // computed-bearing type as procedure input.
    let schema = parse(
        "type Widget {\n  label String\n  slug String @computed\n}\n\
         type Shelf {\n  widgets Widget[]\n}\n\
         procedure get(label: String): Shelf\n  @allow(true)",
    );
    let error = procedure_output_schema(&schema, &schema.procedures[0], None).unwrap_err();
    assert_eq!(refused_type(&error), "Widget");
    assert!(
        error
            .to_string()
            .ends_with("field `Shelf.widgets` → field `Widget.slug`)")
    );
}

#[test]
fn decimal_without_a_backend_is_refused() {
    let schema = parse("procedure total(amount: Decimal): Boolean");
    let error = procedure_input_schema(&schema, &schema.procedures[0], None).unwrap_err();
    assert!(matches!(
        error,
        JsonSchemaError::MissingDecimalBackend { .. }
    ));
    assert!(error.to_string().contains("`Decimal`"));
}
