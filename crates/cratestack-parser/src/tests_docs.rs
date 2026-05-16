#![cfg(test)]

use super::parse_schema;

#[test]
fn preserves_leading_doc_comments_on_declarations_and_fields() {
    let schema = parse_schema(
        r#"
/// User docs
model User {
  /// Identifier docs
  id Int @id
  /// Email docs
  email String
}

/// Feed docs
procedure getFeed(): User
"#,
    )
    .expect("schema with docs should parse");

    assert_eq!(schema.models[0].docs, vec!["User docs".to_owned()]);
    assert_eq!(
        schema.models[0].fields[0].docs,
        vec!["Identifier docs".to_owned()]
    );
    assert_eq!(
        schema.models[0].fields[1].docs,
        vec!["Email docs".to_owned()]
    );
    assert_eq!(schema.procedures[0].docs, vec!["Feed docs".to_owned()]);
}

#[test]
fn attaches_param_docs_and_precise_spans_to_procedure_args() {
    let source = r#"
/// Feed docs
/// @param limit Maximum items to fetch
procedure getFeed(limit: Int): Int
"#;
    let schema = parse_schema(source).expect("schema with parameter docs should parse");
    let arg = &schema.procedures[0].args[0];

    assert_eq!(schema.procedures[0].docs, vec!["Feed docs".to_owned()]);
    assert_eq!(arg.docs, vec!["Maximum items to fetch".to_owned()]);
    assert_eq!(&source[arg.span.start..arg.span.end], "limit: Int");
}
