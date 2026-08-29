//! Macro-time coverage for `field in [...]` / `field not in [...]` set
//! membership in `@@allow`/`@@deny` read policies (issue #666's
//! remaining half, after the equality arm landed in
//! [`super::enum_literal`]). Same harness as [`super::tests_enum_literal`]:
//! parse a fixture schema with the real parser, lower its policy, and
//! inspect the emitted tokens.
//!
//! Unit coverage of the term splitter itself (the `join` false-positive
//! guard, quoted commas, empty lists) lives in [`super::in_list`]'s own
//! `tests` module. What is proved here is the *lowering*: that the
//! syntax reaches `FieldInLiterals` rather than being mis-parsed as an
//! unknown field, and that each element is still validated as a real
//! variant.
//!
//! Runtime SQL coverage — that `IN` actually restricts rows — is in
//! `cratestack-sqlx/src/tests_read_policy_field_predicates.rs` (render
//! shape and bind-slot accounting, no database) and
//! `cratestack-pg/tests/policy_db_enum_in_list.rs` (against a real
//! Postgres). A compiling test here proves codegen accepts the syntax
//! and nothing more.

use super::generate_policies_for_action;

fn asset_schema(expression: &str) -> String {
    format!(
        r#"
enum AssetPurpose {{
  product_image
  product_thumbnail
  kyc_document_front
}}

model Asset {{
  id Int @id
  purpose AssetPurpose
  status String

  @@allow("read", {expression})
}}
"#
    )
}

fn lower(schema: &str) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let parsed = cratestack_parser::parse_schema(schema).expect("fixture schema should parse");
    let model = parsed.models.first().expect("fixture declares a model");
    generate_policies_for_action(
        model,
        &parsed.models,
        &parsed.types,
        &parsed.enums,
        parsed.auth.as_ref(),
        "read",
    )
}

fn lowered_tokens(expression: &str) -> String {
    lower(&asset_schema(expression))
        .unwrap_or_else(|error| panic!("`{expression}` should compile, got: {error}"))
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The headline case from #666: a required enum field against a set of
/// variants lowers to one flat `FieldInLiterals`, not a nested `Or` of
/// equalities.
#[test]
fn enum_in_list_lowers_to_a_single_field_in_literals() {
    let lowered = lowered_tokens("purpose in [product_image, product_thumbnail]");
    assert!(
        lowered.contains("FieldInLiterals"),
        "expected FieldInLiterals predicate, got: {lowered}"
    );
    assert!(
        !lowered.contains("Or"),
        "an `in` list must lower flat, not desugar to an Or tree, got: {lowered}"
    );
    for variant in ["product_image", "product_thumbnail"] {
        assert!(
            lowered.contains(&format!("\"{variant}\"")),
            "expected `{variant}` in the emitted values, got: {lowered}"
        );
    }
}

/// `not in` lowers to the negated variant.
#[test]
fn enum_not_in_list_lowers_to_field_not_in_literals() {
    let lowered = lowered_tokens("purpose not in [kyc_document_front]");
    assert!(
        lowered.contains("FieldNotInLiterals"),
        "expected FieldNotInLiterals predicate, got: {lowered}"
    );
}

/// The shape is not enum-only: it reuses `parse_policy_literal`, so
/// every type the equality arm accepts works here too.
#[test]
fn string_in_list_lowers_the_same_way() {
    let lowered = lowered_tokens(r#"status in ["draft", "review"]"#);
    assert!(lowered.contains("FieldInLiterals"), "got: {lowered}");
    assert!(lowered.contains("\"draft\""), "got: {lowered}");
    assert!(lowered.contains("\"review\""), "got: {lowered}");
}

/// A single-element list is legal — it is `==` written differently, and
/// rejecting it would be a gratuitous special case.
#[test]
fn a_single_element_list_is_accepted() {
    assert!(lowered_tokens("purpose in [product_image]").contains("FieldInLiterals"));
}

/// Decisive test: element validation is not skipped for the list shape.
/// One bad variant among good ones must fail the whole term, otherwise
/// a typo silently narrows a policy.
#[test]
fn an_unknown_variant_anywhere_in_the_list_is_a_compile_error() {
    let result = lower(&asset_schema(
        "purpose in [product_image, not_a_real_variant]",
    ));
    let Err(message) = result else {
        panic!("an unknown variant in an `in` list must not compile");
    };
    assert!(
        message.contains("not_a_real_variant"),
        "the error should name the offending variant, got: {message}"
    );
}

/// `field in []` is rejected. It would render as a constant `FALSE`
/// dressed up as a policy, and SQL has no valid `IN ()` form.
#[test]
fn an_empty_list_is_a_compile_error() {
    let Err(message) = lower(&asset_schema("purpose in []")) else {
        panic!("an empty `in` list must not compile");
    };
    assert!(message.contains("at least one value"), "got: {message}");
}

/// Decisive test for the whole-word guard in `in_list::strip_keyword`,
/// written against the case where removing it is dangerous rather than
/// merely confusing: a bracketed term with no `in` keyword. Without the
/// guard, `join [...]` has the trailing `in` stripped off the *field
/// name*, leaving `jo` — and if `jo` is also a real column the
/// malformed term compiles into a policy that gates on the wrong data.
///
/// Both `jo` and `join` are declared on purpose. With only `join` the
/// mis-strip fails to resolve and the test passes for the wrong reason
/// — as an earlier version of it did, because the well-formed `origin
/// in [...]` never exercises the guard at all (`"origin in"
/// .strip_suffix("in")` removes the standalone keyword, not the
/// field's tail).
#[test]
fn a_bracket_term_without_the_in_keyword_is_rejected_not_mis_resolved() {
    let schema = r#"
model Route {
  id Int @id
  jo String
  join String

  @@allow("read", join ["berlin"])
}
"#;
    let Err(message) = lower(schema) else {
        panic!(
            "`join [\"berlin\"]` is malformed and must not compile — it silently became a policy on `jo`"
        );
    };
    assert!(
        message.contains("join"),
        "the error should quote the term the author wrote, got: {message}"
    );
}

/// The mirror of the above: a well-formed `in` term on a field whose
/// name ends in the keyword keeps its whole name.
#[test]
fn an_in_list_on_a_field_whose_name_ends_in_in_uses_the_whole_name() {
    let schema = r#"
model Route {
  id Int @id
  origin String

  @@allow("read", origin in ["berlin"])
}
"#;
    let lowered = lower(schema)
        .expect("`origin in [...]` must compile")
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        lowered.contains("column : \"origin\""),
        "expected the predicate to gate on `origin`, got: {lowered}"
    );
}
