//! "Copy Rust query" snippet generator.
//!
//! Given a model name and a primary-key value, returns a Rust string
//! that calls into the macro-generated delegate to fetch the same row.
//! Studio surfaces this in the record drawer so users can drop the
//! snippet directly into a service crate.

use cratestack_core::Schema;

use crate::data::DataError;
use crate::data::postgres::resolve_model;

/// Render a Rust snippet that reads the model row by primary key.
/// Quoting follows the PK-type rule: text-shaped keys get string
/// literals, numeric keys get the unquoted value.
pub fn rust_find_unique(
    schema: &Schema,
    model: &str,
    pk_value: &str,
) -> Result<String, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let pk_field = resolved
        .fields
        .iter()
        .find(|f| f.attributes.iter().any(|a| a.raw.starts_with("@id")))
        .expect("resolve_model returns NoPrimaryKey otherwise");

    let delegate = snake_case(&resolved.name);
    let pk_literal = pk_literal_for(&pk_field.ty.name, pk_value, info.pk_cast);

    Ok(format!(
        "let row = cool.{delegate}()\n    \
         .find_unique({pk_literal})\n    \
         .run(&ctx)\n    \
         .await?;\n"
    ))
}

fn pk_literal_for(
    _scalar: &str,
    pk_value: &str,
    cast: crate::data::postgres::PkCast,
) -> String {
    use crate::data::postgres::PkCast;
    match cast {
        PkCast::BigInt => format!("{pk_value}_i64"),
        PkCast::Text => format!("\"{}\".to_owned()", escape_str(pk_value)),
    }
}

fn escape_str(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn snake_case(value: &str) -> String {
    let mut out = String::new();
    for (i, c) in value.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            for lower in c.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Schema {
        cratestack_parser::parse_schema(text).expect("schema parses")
    }

    #[test]
    fn renders_string_pk_with_owned_literal() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let snippet = rust_find_unique(&schema, "Post", "abc-123").expect("ok");
        assert!(snippet.contains("cool.post()"), "{snippet}");
        assert!(
            snippet.contains(".find_unique(\"abc-123\".to_owned())"),
            "{snippet}"
        );
    }

    #[test]
    fn renders_int_pk_with_typed_literal() {
        let schema = parse(
            r#"
                model Customer {
                  id Int @id
                  email String
                }
            "#,
        );
        let snippet = rust_find_unique(&schema, "Customer", "42").expect("ok");
        assert!(snippet.contains(".find_unique(42_i64)"), "{snippet}");
    }

    #[test]
    fn escapes_quotes_in_string_pk() {
        let schema = parse(
            r#"
                model Note {
                  id String @id
                  body String
                }
            "#,
        );
        let snippet = rust_find_unique(&schema, "Note", "a\"b\\c").expect("ok");
        assert!(snippet.contains(r#"a\"b\\c"#), "{snippet}");
    }

    #[test]
    fn unknown_model_errors() {
        let schema = parse(
            r#"
                model Post {
                  id String @id
                  title String
                }
            "#,
        );
        let error = rust_find_unique(&schema, "Nope", "1").expect_err("missing model errors");
        assert!(matches!(error, DataError::UnknownModel { .. }));
    }

    #[test]
    fn snake_case_handles_camel_and_pascal() {
        assert_eq!(snake_case("Post"), "post");
        assert_eq!(snake_case("OrderItem"), "order_item");
        assert_eq!(snake_case("user"), "user");
    }
}
