#![cfg(test)]

use super::parse_schema;

#[test]
fn expands_mixin_fields_via_model_use_attribute() {
    let schema = parse_schema(
        r#"
mixin Timestamps {
  createdAt DateTime
  updatedAt DateTime
}

model Post {
  @use(Timestamps)
  id Int @id
  title String
}
"#,
    )
    .expect("mixin usage should parse");

    let post = &schema.models[0];
    assert_eq!(
        post.fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        vec!["createdAt", "updatedAt", "id", "title"]
    );
    assert!(post.attributes.is_empty());
}

#[test]
fn model_local_fields_override_mixin_fields() {
    let schema = parse_schema(
        r#"
mixin Timestamps {
  createdAt DateTime
}

model Post {
  @use(Timestamps)
  id Int @id
  createdAt DateTime?
}
"#,
    )
    .expect("model field override should parse");

    let post = &schema.models[0];
    assert_eq!(post.fields.len(), 2);
    assert_eq!(post.fields[1].name, "createdAt");
    assert_eq!(
        post.fields[1].ty.arity,
        cratestack_core::TypeArity::Optional
    );
}

#[test]
fn rejects_model_use_with_unknown_mixin() {
    let error = parse_schema(
        r#"
model Post {
  @use(UnknownMixin)
  id Int @id
}
"#,
    )
    .expect_err("unknown mixin should fail");

    assert!(error.to_string().contains("unknown mixin `UnknownMixin`"));
}

#[test]
fn rejects_mixin_id_fields() {
    let error = parse_schema(
        r#"
mixin Identity {
  id Int @id
}

model Post {
  @use(Identity)
  title String
}
"#,
    )
    .expect_err("mixin @id should fail");

    assert!(error.to_string().contains("cannot declare @id"));
}
