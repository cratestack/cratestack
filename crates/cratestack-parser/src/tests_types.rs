#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_custom_fields_on_types() {
    let schema = parse_schema(
        r#"
type Image {
  storageKey String
  thumbnailUrl String @custom
}
"#,
    )
    .expect("type custom fields should parse");

    assert_eq!(schema.types[0].fields[1].attributes[0].raw, "@custom");
}

#[test]
fn rejects_custom_fields_on_models() {
    let error = parse_schema(
        r#"
model Image {
  id Int @id
  storageKey String
  thumbnailUrl String @custom
}
"#,
    )
    .expect_err("model custom fields should fail validation");

    assert!(error.to_string().contains(
        "resolver-backed custom fields are currently only supported on `type` declarations"
    ));
}

#[test]
fn parses_built_in_page_return_type() {
    let source = r#"
model Post {
  id Int @id
}

procedure getFeedPage(): Page<Post>
"#;
    let schema = parse_schema(source).expect("schema with Page<T> return should parse");
    let return_type = &schema.procedures[0].return_type;

    assert_eq!(return_type.name, "Page");
    assert_eq!(return_type.generic_args.len(), 1);
    assert_eq!(return_type.generic_args[0].name, "Post");
    assert_eq!(
        &source[return_type.name_span.start..return_type.name_span.end],
        "Page"
    );
    assert_eq!(
        &source[return_type.generic_args[0].name_span.start
            ..return_type.generic_args[0].name_span.end],
        "Post"
    );
}

#[test]
fn rejects_page_return_types_outside_procedure_returns() {
    let error = parse_schema(
        r#"
type Feed {
  posts Page<Post>
}

model Post {
  id Int @id
}
"#,
    )
    .expect_err("Page<T> fields should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supported as a procedure return type")
    );
}

#[test]
fn rejects_page_returns_with_scalar_items() {
    let error = parse_schema(
        r#"
procedure getCounts(): Page<Int>
"#,
    )
    .expect_err("Page<T> with scalar item should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supports declared model or type items")
    );
}

#[test]
fn accepts_decimal_scalar_in_models_and_procedures() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal
  available Decimal?
}

type CreditInput {
  accountId Int
  amount Decimal
}

mutation procedure credit(args: CreditInput): Account
"#,
    )
    .expect("schema with Decimal should parse");

    let balance = &schema.models[0].fields[1];
    assert_eq!(balance.name, "balance");
    assert_eq!(balance.ty.name, "Decimal");
    let amount = &schema.types[0].fields[1];
    assert_eq!(amount.ty.name, "Decimal");
}
