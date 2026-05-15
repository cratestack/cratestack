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
