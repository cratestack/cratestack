#![cfg(test)]

use super::parse_schema;

#[test]
fn vector_field_parses_with_declared_extension() {
    let schema = parse_schema(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(1536)
}
"#,
    )
    .expect("Vector(n) field should parse when pgvector extension is declared");

    let embedding = &schema.models[0].fields[1];
    assert_eq!(embedding.name, "embedding");
    assert_eq!(embedding.ty.name, "Vector");
    assert_eq!(embedding.ty.int_args, vec![1536]);
    assert_eq!(embedding.ty.vector_dim(), Some(1536));
}

#[test]
fn optional_vector_field_parses() {
    let schema = parse_schema(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(3)?
}
"#,
    )
    .expect("optional Vector(n) field should parse");

    let embedding = &schema.models[0].fields[1];
    assert_eq!(embedding.ty.arity, cratestack_core::TypeArity::Optional);
    assert_eq!(embedding.ty.vector_dim(), Some(3));
}

#[test]
fn vector_field_without_declared_extension_is_a_validation_error() {
    let error = parse_schema(
        r#"
model Document {
  id Int @id
  embedding Vector(1536)
}
"#,
    )
    .expect_err("Vector(n) without `extension pgvector { }` should fail validation");

    assert!(
        error.to_string().contains("requires `extension pgvector"),
        "error should point at the missing extension declaration, got: {error}",
    );
}

#[test]
fn vector_field_rejects_zero_dimension() {
    let error = parse_schema(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector(0)
}
"#,
    )
    .expect_err("Vector(0) should be rejected");

    assert!(
        error
            .to_string()
            .contains("dimension must be greater than zero"),
        "error should explain the dimension must be positive, got: {error}",
    );
}

#[test]
fn vector_field_rejects_list_arity() {
    let error = parse_schema(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embeddings Vector(8)[]
}
"#,
    )
    .expect_err("Vector(n)[] should be rejected");

    assert!(
        error.to_string().contains("cannot be list-valued"),
        "error should explain list-valued vectors aren't supported, got: {error}",
    );
}

#[test]
fn vector_field_rejects_missing_dimension() {
    let error = parse_schema(
        r#"
extension pgvector {
}

model Document {
  id Int @id
  embedding Vector
}
"#,
    )
    .expect_err("bare `Vector` with no dimension should be rejected");

    assert!(
        error
            .to_string()
            .contains("requires exactly one integer dimension argument"),
        "error should explain the missing dimension argument, got: {error}",
    );
}

#[test]
fn parametric_argument_on_non_vector_type_is_rejected() {
    let error = parse_schema(
        r#"
model Document {
  id Int @id
  weight Float(8)
}
"#,
    )
    .expect_err("a parenthesized argument on a non-Vector type should be rejected");

    assert!(
        error
            .to_string()
            .contains("does not accept a parametric argument"),
        "error should name the offending type, got: {error}",
    );
}

#[test]
fn vector_field_rejected_in_procedure_signature() {
    let error = parse_schema(
        r#"
extension pgvector {
}

procedure search(embedding: Vector(8)): Int
"#,
    )
    .expect_err("Vector(n) is not supported in procedure signatures in this release");

    assert!(
        error
            .to_string()
            .contains("only supported on model/mixin/type/auth fields"),
        "error should explain Vector(n) isn't supported here, got: {error}",
    );
}
