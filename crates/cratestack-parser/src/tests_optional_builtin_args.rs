//! What the parser admits as an optional built-in argument, which the
//! compat classifier (`cratestack-core`, `client_contract/compat_op.rs`)
//! relies on (review of cratestack#1132, S1).
//!
//! The generated `Args` struct types `FindMany<T>` as a bare (never
//! `Option<_>`) field with no `serde(default)`, whatever arity the schema
//! wrote. The parser accepts `FindMany<T>?`, so the classifier refuses it as
//! an added argument. `Page<T>?` never reaches the classifier: the parser
//! refuses `Page<T>` in any argument position.

use crate::parse_schema;

fn with_arg(arg: &str) -> Result<cratestack_core::Schema, String> {
    parse_schema(&format!(
        "model Post {{\n  id Int @id\n}}\n\nprocedure search(q: String, page: {arg}): Int\n"
    ))
    .map_err(|e| e.to_string())
}

#[test]
fn the_parser_accepts_an_optional_find_many_argument() {
    let schema = with_arg("FindMany<Post>?").expect("parses");
    assert_eq!(schema.procedures[0].args[1].ty.name, "FindMany");
}

#[test]
fn the_parser_refuses_a_page_argument_optional_or_not() {
    for arg in ["Page<Post>?", "Page<Post>"] {
        let error = with_arg(arg).expect_err("Page<T> is return-only");
        assert!(
            error.contains("only supported as a procedure return type"),
            "{error}"
        );
    }
}
