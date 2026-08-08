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

// ---------------------------------------------------------------
// SPIKE (`spike/b1-internal-actions`): `@@internal(...)` route
// suppression.
//
// The property under test is the *separation* the spike exists to
// prove: an action can be declared internal — no REST route — while
// its `@@allow` policy is still compiled and still enforced. These
// tests therefore always assert both halves together; asserting only
// "no route" would pass just as well for a feature that silently
// dropped the policy.
// ---------------------------------------------------------------

use crate::policy::generate_policies_for_action;

/// The model the downstream services actually want to write: readable
/// by its owner over REST, writable only by server code, with the
/// write rule still stated in the schema.
fn device_schema(internal_attribute: &str) -> String {
    format!(
        r#"
model Device {{
  id Int @id
  subjectId String

  @@allow("read", auth() != null)
  @@allow("update", auth() != null)
  {internal_attribute}
}}
"#
    )
}

fn routes_for(internal_attribute: &str) -> String {
    let model = parse_first_model(&device_schema(internal_attribute));
    generate_model_axum_routes(&model).to_string()
}

fn update_policy_count(internal_attribute: &str) -> usize {
    let model = parse_first_model(&device_schema(internal_attribute));
    generate_policies_for_action(&model, std::slice::from_ref(&model), &[], None, "update")
        .expect("update policy should still compile for an internal action")
        .len()
}

/// Baseline: with no `@@internal`, every verb is mounted. Pins what
/// the suppression tests are measured against.
#[test]
fn without_internal_attribute_every_verb_is_mounted() {
    let routes = routes_for("");
    for verb in ["get", "post", "patch", "delete"] {
        assert!(
            routes.contains(verb),
            "expected `{verb}` to be mounted, got: {routes}"
        );
    }
}

/// (a) The headline case. `@@internal("update")` drops `PATCH` and
/// nothing else — and the `update` policy is still generated.
#[test]
fn internal_update_suppresses_patch_but_keeps_the_policy() {
    let routes = routes_for(r#"@@internal("update")"#);

    assert!(
        !routes.contains("patch"),
        "PATCH must not be mounted for an @@internal(\"update\") action, got: {routes}"
    );
    for verb in ["get", "post", "delete"] {
        assert!(
            routes.contains(verb),
            "`{verb}` must be unaffected by @@internal(\"update\"), got: {routes}"
        );
    }
    // Both route paths still exist — only one verb went away.
    assert!(routes.contains("/devices"));
    assert!(routes.contains("/devices/{id}"));

    // The load-bearing half: the policy survives route suppression.
    assert_eq!(
        update_policy_count(r#"@@internal("update")"#),
        1,
        "@@internal must suppress the route only — the update policy must still compile"
    );
    assert_eq!(
        update_policy_count(""),
        update_policy_count(r#"@@internal("update")"#),
        "@@internal must not change how many update policies are generated"
    );
}

/// When every verb on a path is suppressed, the `.route(...)` for that
/// path is omitted rather than mounted with an empty `MethodRouter`
/// (which would answer 405 instead of 404 and still show up in the
/// route table).
#[test]
fn suppressing_every_verb_on_a_path_drops_the_route_entirely() {
    let routes = routes_for(r#"@@internal("detail", "update", "delete")"#);
    assert!(
        !routes.contains("/devices/{id}"),
        "detail path must not be registered at all, got: {routes}"
    );
    assert!(
        routes.contains("/devices"),
        "collection path must be unaffected, got: {routes}"
    );
}

/// `@@internal("all")` leaves a model with a policy surface but no
/// REST surface whatsoever — the "ORM-only model" shape.
#[test]
fn internal_all_suppresses_every_route() {
    let routes = routes_for(r#"@@internal("all")"#);
    assert!(
        routes.trim().is_empty(),
        "@@internal(\"all\") should emit no routes, got: {routes}"
    );
    assert_eq!(
        update_policy_count(r#"@@internal("all")"#),
        1,
        "policies must survive even when no route is mounted"
    );
}

/// `read` is the alias covering both `list` and `detail`, matching how
/// `@@allow("read", ...)` already lands in both descriptor slots.
#[test]
fn internal_read_suppresses_both_list_and_detail_gets() {
    let routes = routes_for(r#"@@internal("read")"#);
    assert!(
        !routes.contains("get"),
        "both GETs should be suppressed by @@internal(\"read\"), got: {routes}"
    );
    assert!(routes.contains("post"), "POST must survive: {routes}");
    assert!(routes.contains("patch"), "PATCH must survive: {routes}");
}

#[test]
fn unknown_internal_action_is_a_schema_error() {
    let schema = device_schema(r#"@@internal("upsert")"#);
    let error = cratestack_parser::parse_schema(&schema)
        .expect_err("an unknown @@internal action should not validate");
    assert!(
        format!("{error:?}").contains("upsert"),
        "error should name the offending action: {error:?}"
    );
}
