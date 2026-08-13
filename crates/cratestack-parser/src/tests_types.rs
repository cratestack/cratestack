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
fn rejects_type_block_used_as_model_field_storage_type() {
    // Regression test for #230: a model field typed with a user-declared
    // `type` block used to pass `check` even though the Postgres emitter
    // renders a composite type name that no `CREATE TYPE` ever backs, and
    // the schema macros panic at expansion for the same shape. This is
    // the issue's own `composite_probe.cstack` reproduction, verbatim
    // (minus the `datasource`/`env()` bits, which need no special
    // handling here — `validate_datasource` only checks the `provider`
    // literal).
    let source = r#"
auth Operator {
  id Int
}

type Address {
  street String
  city String
}

model Person {
  id Int @id
  addr Address

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}
"#;

    let error =
        parse_schema(source).expect_err("a `type` block used as a model field should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("Person") && message.contains("addr") && message.contains("Address"),
        "error should name the model, field, and type: {message}"
    );
    assert!(
        message.contains("cannot use `type Address` as its storage type"),
        "error should explain why `type` blocks can't be a column: {message}"
    );

    // The diagnostic must carry a real span pointing at the offending
    // field, not just a bare message — this is what makes it a spanned
    // parser error rather than a generic failure.
    let span = error.span();
    assert!(
        span.start < span.end && span.end <= source.len(),
        "span should be a non-empty range within the source: {span:?}"
    );
    let spanned_text = &source[span];
    assert!(
        spanned_text.contains("addr") && spanned_text.contains("Address"),
        "span should point at the `addr Address` field declaration, got: {spanned_text:?}"
    );
}

/// A `type` block referencing a `model` (#137) is a different, legitimate
/// use of `type` and must keep working — this is not what #230 rejects.
/// See `crates/cratestack-pg/tests/type_block_model_reference.rs` /
/// `crates/cratestack-sqlite/tests/type_block_model_reference.rs` for the
/// end-to-end codegen coverage; this is the parser-level counterpart.
#[test]
fn accepts_type_block_field_referencing_a_model() {
    let schema = parse_schema(
        r#"
model SomeModel {
  id Int @id
  name String
}

type ApiKeySecret {
  model SomeModel
  secret String
}
"#,
    )
    .expect("a `type` block field referencing a `model` should still parse");

    assert_eq!(schema.types[0].fields[0].ty.name, "SomeModel");
}

#[test]
fn parses_built_in_page_input_procedure_argument() {
    let source = r#"
model Post {
  id Int @id
}

procedure listPosts(page: PageInput): Post[]
"#;
    let schema = parse_schema(source).expect("schema with PageInput arg should parse");
    let arg_type = &schema.procedures[0].args[0].ty;

    assert_eq!(arg_type.name, "PageInput");
    assert!(arg_type.generic_args.is_empty());
}

#[test]
fn rejects_page_input_outside_procedure_arguments() {
    let error = parse_schema(
        r#"
type Feed {
  page PageInput
}
"#,
    )
    .expect_err("PageInput fields should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supported as a procedure argument type")
    );
}

#[test]
fn rejects_page_input_as_procedure_return_type() {
    let error = parse_schema(
        r#"
procedure getPage(): PageInput
"#,
    )
    .expect_err("PageInput return type should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supported as a procedure argument type")
    );
}

#[test]
fn rejects_list_valued_page_input() {
    let error = parse_schema(
        r#"
procedure listPosts(pages: PageInput[]): Int
"#,
    )
    .expect_err("list-valued PageInput should fail validation");

    assert!(error.to_string().contains("cannot be list-valued"));
}

#[test]
fn parses_built_in_find_many_procedure_argument() {
    let source = r#"
model Post {
  id Int @id
}

procedure searchPosts(query: FindMany<Post>): Post[]
"#;
    let schema = parse_schema(source).expect("schema with FindMany<T> arg should parse");
    let arg_type = &schema.procedures[0].args[0].ty;

    assert_eq!(arg_type.name, "FindMany");
    assert_eq!(arg_type.generic_args.len(), 1);
    assert_eq!(arg_type.generic_args[0].name, "Post");
}

#[test]
fn rejects_find_many_outside_procedure_arguments() {
    let error = parse_schema(
        r#"
model Post {
  id Int @id
}

type Feed {
  query FindMany<Post>
}
"#,
    )
    .expect_err("FindMany<T> fields should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supported as a procedure argument type")
    );
}

#[test]
fn rejects_find_many_as_procedure_return_type() {
    let error = parse_schema(
        r#"
model Post {
  id Int @id
}

procedure searchPosts(): FindMany<Post>
"#,
    )
    .expect_err("FindMany<T> return type should fail validation");

    assert!(
        error
            .to_string()
            .contains("only supported as a procedure argument type")
    );
}

#[test]
fn rejects_find_many_over_a_type_block() {
    let error = parse_schema(
        r#"
type Feed {
  posts String
}

procedure searchFeeds(query: FindMany<Feed>): Int
"#,
    )
    .expect_err("FindMany<T> over a `type` block should fail validation");

    assert!(error.to_string().contains("only supports declared models"));
}

#[test]
fn rejects_find_many_over_a_scalar() {
    let error = parse_schema(
        r#"
procedure searchCounts(query: FindMany<Int>): Int
"#,
    )
    .expect_err("FindMany<T> over a scalar should fail validation");

    assert!(error.to_string().contains("only supports declared models"));
}

#[test]
fn rejects_list_valued_find_many() {
    let error = parse_schema(
        r#"
model Post {
  id Int @id
}

procedure searchPosts(queries: FindMany<Post>[]): Int
"#,
    )
    .expect_err("list-valued FindMany<T> should fail validation");

    assert!(error.to_string().contains("cannot be list-valued"));
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
