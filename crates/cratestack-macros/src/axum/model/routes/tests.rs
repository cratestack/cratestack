//! cratestack#345 regression guard: this is the server's real,
//! load-bearing REST route derivation — the one `cratestack-client-typescript`
//! and `cratestack-client-dart`'s generators must match exactly. Pin the
//! literal route strings this function emits for schema names beyond the
//! happy-path PascalCase case (an underscore boundary, consecutive
//! uppercase, etc.) so a future change can't silently drift the server's
//! actual routes away from what the client generators independently
//! confirm they produce (see the mirrored tables in
//! `cratestack-client-typescript`'s and `cratestack-client-dart`'s own
//! route tests, and the canonical algorithm's own table-driven test in
//! `cratestack_core::route_naming`).

use super::generate_model_axum_routes;

fn parse_first_model(source: &str) -> cratestack_core::Model {
    cratestack_parser::parse_schema(source)
        .expect("fixture schema should parse and validate")
        .models
        .remove(0)
}

fn generated_route_literals(model_name: &str) -> (String, String) {
    let schema = format!(
        r#"
model {model_name} {{
  id Int @id
}}
"#
    );
    let model = parse_first_model(&schema);
    let generated = generate_model_axum_routes(&model).to_string();
    // The two route strings are the only two string literals `quote!`
    // emits in this function's output; pull them out rather than
    // re-deriving them, so this test only exercises the real codegen
    // path, not a second copy of the algorithm.
    let literals = generated
        .split('"')
        .enumerate()
        .filter_map(|(index, chunk)| (index % 2 == 1).then_some(chunk))
        .collect::<Vec<_>>();
    (literals[0].to_owned(), literals[1].to_owned())
}

#[test]
fn plain_pascal_case_name_routes() {
    let (list_route, detail_route) = generated_route_literals("UserGroup");
    assert_eq!(list_route, "/user_groups");
    assert_eq!(detail_route, "/user_groups/{id}");
}

/// The exact case cratestack#345 reports: a model name containing a
/// literal `_` must not be silently tokenized the way a natural-language
/// word-splitter would. The pre-existing `_` passes through unchanged,
/// and a second `_` is still inserted before the uppercase `G` — hence
/// the double underscore.
#[test]
fn underscore_containing_name_routes() {
    let (list_route, detail_route) = generated_route_literals("User_Group");
    assert_eq!(list_route, "/user__groups");
    assert_eq!(detail_route, "/user__groups/{id}");
}

#[test]
fn consecutive_uppercase_run_routes() {
    let (list_route, detail_route) = generated_route_literals("HTTPServer");
    assert_eq!(list_route, "/h_t_t_p_servers");
    assert_eq!(detail_route, "/h_t_t_p_servers/{id}");
}

#[test]
fn already_snake_case_name_is_unchanged() {
    let (list_route, detail_route) = generated_route_literals("already_snake");
    assert_eq!(list_route, "/already_snakes");
    assert_eq!(detail_route, "/already_snakes/{id}");
}

/// cratestack#743: a model with no `@@internal` attribute must still
/// route every verb on both paths — the byte-identical-to-today
/// guarantee the design's acceptance criteria requires. This is the
/// case `just regen-examples --check` pins for the generated clients;
/// this test pins the same guarantee for the server's own route
/// derivation.
#[test]
fn model_without_internal_attribute_routes_every_verb() {
    let model = parse_first_model(
        r#"
model Widget {
  id String @id
}
"#,
    );
    let generated = generate_model_axum_routes(&model).to_string();
    assert!(generated.contains("axum :: routing :: get"));
    assert!(generated.contains("axum :: routing :: post"));
    assert!(generated.contains("axum :: routing :: patch"));
    assert!(generated.contains("axum :: routing :: delete"));
    assert!(generated.contains("widgets"));
}

/// cratestack#743 negative control's macro-level half: suppressing
/// `create` must remove the `POST` `MethodRouter` fragment from the
/// list path while leaving `GET` (list) and every detail-path verb
/// routed — proving the omission is scoped to the one suppressed verb,
/// not the whole path.
#[test]
fn internal_create_omits_post_but_keeps_list_and_detail() {
    let model = parse_first_model(
        r#"
model Widget {
  id String @id

  @@internal("create")
}
"#,
    );
    let generated = generate_model_axum_routes(&model).to_string();
    assert!(
        generated.contains("axum :: routing :: get"),
        "list GET must survive: {generated}"
    );
    assert!(
        !generated.contains("axum :: routing :: post"),
        "create POST must be omitted: {generated}"
    );
    assert!(
        generated.contains("axum :: routing :: patch"),
        "detail PATCH must survive: {generated}"
    );
    assert!(
        generated.contains("axum :: routing :: delete"),
        "detail DELETE must survive: {generated}"
    );
}

/// cratestack#743: `@@internal("all")` suppresses every verb on both
/// paths, so neither `.route(...)` call — and therefore neither path
/// literal — should be emitted at all. Proves the "every verb on a
/// path suppressed ⇒ the whole path is never registered" half of the
/// design (§3/§4): the path falls through to axum's own default 404,
/// not a registered-but-empty route.
#[test]
fn internal_all_omits_both_paths_entirely() {
    let model = parse_first_model(
        r#"
model Widget {
  id String @id

  @@internal("all")
}
"#,
    );
    let generated = generate_model_axum_routes(&model).to_string();
    assert!(
        !generated.contains(".route("),
        "no path should be registered when every verb is suppressed: {generated}"
    );
}
