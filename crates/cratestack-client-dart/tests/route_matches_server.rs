//! cratestack#345 regression guard: the generated Dart client's REST route
//! for a model must match the server's real Axum route registration
//! exactly, including for model names the old `split_words`-based
//! `to_snake_case` in `crate::idents` diverged on (a literal `_` boundary).
//!
//! Before the fix, this crate derived routes with a natural-language
//! word-tokenizer that treated `_` as a separator to drop — for a model
//! named `User_Group` it produced `/user_groups`, the exact same route as
//! plain `UserGroup`, while the server (`cratestack-macros::shared::
//! to_snake_case`, which never tokenizes on `_`) registered `/user__groups`.
//! That's a guaranteed 404 for `User_Group` and a route collision with
//! `UserGroup`. Both are now derived through the single canonical
//! `cratestack_core::route_naming::model_route_segment`, so they can't
//! drift apart again.

use cratestack_client_dart::{DartGeneratorConfig, generate_package};

const UNDERSCORE_MODEL_SCHEMA: &str = r#"
model UserGroup {
  id Int @id
}

model User_Group {
  id Int @id
}
"#;

fn generated_apis_source() -> String {
    let schema =
        cratestack_parser::parse_schema(UNDERSCORE_MODEL_SCHEMA).expect("fixture should parse");
    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");
    package
        .files
        .iter()
        .find(|file| file.file_name == "lib/src/apis.dart")
        .expect("apis.dart should be generated")
        .contents
        .clone()
}

#[test]
fn underscore_containing_model_route_matches_the_canonical_server_algorithm() {
    let apis = generated_apis_source();

    // The canonical algorithm is exactly what the server's own Axum route
    // registration uses (`cratestack-macros::shared::to_snake_case` /
    // `pluralize`, re-exported from the same `cratestack_core::route_naming`
    // module) — asserting against it, rather than a hardcoded literal,
    // keeps this test tied to the real cross-crate contract.
    let user_group_route = format!(
        "'/{}'",
        cratestack_core::route_naming::model_route_segment("UserGroup")
    );
    let user_underscore_group_route = format!(
        "'/{}'",
        cratestack_core::route_naming::model_route_segment("User_Group")
    );

    assert_ne!(
        user_group_route, user_underscore_group_route,
        "UserGroup and User_Group must not collide onto the same route"
    );
    assert!(
        apis.contains(&user_group_route),
        "expected apis.dart to reference {user_group_route}, got:\n{apis}"
    );
    assert!(
        apis.contains(&user_underscore_group_route),
        "expected apis.dart to reference {user_underscore_group_route}, got:\n{apis}"
    );

    // Pin the literal values from cratestack#345's own repro table so a
    // change to the canonical algorithm's behavior for these specific
    // inputs is visible here too, not just in `cratestack-core`'s own
    // tests.
    assert!(apis.contains("'/user_groups'"));
    assert!(apis.contains("'/user__groups'"));
}
