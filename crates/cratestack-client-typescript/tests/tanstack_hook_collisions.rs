//! cratestack#802: `--tanstack` must refuse a schema whose procedure hook
//! name collides with a derived model hook name, the way #777/#778 made
//! `--swr` refuse the analogous free-function collision.
//!
//! Both hook families are emitted into the same `src/react-query.ts`, so a
//! collision here is a same-file duplicate declaration rather than #777's
//! barrel-level TS2308 — a sharper failure, and one `export *`
//! de-duplication cannot mask. Real `tsc` reports TS2393 + TS2323 on it
//! (measured with the check disabled; #802 predicted TS2300, a different
//! code).
//!
//! Four tests, three of which exist to stop this check being satisfied the
//! wrong way:
//!
//! 1. the mutation-suffix collision is rejected,
//! 2. the query-suffix collision is rejected — a separate fixture, because
//!    the check returns on the first collision and one schema can only
//!    ever prove one,
//! 3. the same query collision is rejected on **RPC** transport too, since
//!    `rest-react-query.ts.j2` and `rpc-react-query.ts.j2` are separate
//!    templates emitting the same two families (transport-parity rule),
//! 4. the default and `--swr` layouts still **accept** these schemas —
//!    without this, "reject everything" would pass every test above.
//!
//! Test 4 is the one that encodes decision spike #317: a schema never
//! generated with `--tanstack` must not be constrained by `--tanstack`'s
//! naming scheme.

use cratestack_client_typescript::{
    TypeScriptGeneratorConfig, TypeScriptGeneratorError, generate_package,
};

const MUTATION_FIXTURE: &str = "tests/fixtures/tanstack_mutation_hook_collision.cstack";
const QUERY_FIXTURE: &str = "tests/fixtures/tanstack_query_hook_collision.cstack";
const QUERY_FIXTURE_RPC: &str = "tests/fixtures/tanstack_query_hook_collision_rpc.cstack";

fn parse(fixture_path: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema_file(fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"))
}

fn tanstack_config() -> TypeScriptGeneratorConfig {
    TypeScriptGeneratorConfig {
        package_name: "tanstack-fixture-client".to_owned(),
        tanstack: true,
        ..TypeScriptGeneratorConfig::default()
    }
}

/// Asserts the typed error, not just that generation failed — a schema can
/// fail for unrelated reasons (a parse error, a composite PK) and an
/// `is_err()` assertion would happily pass on any of them.
#[track_caller]
fn expect_collision(
    fixture_path: &str,
    expected_procedure: &str,
    expected_identifier: &str,
    expected_operation: &str,
) {
    let schema = parse(fixture_path);
    let error = generate_package(&schema, &tanstack_config())
        .expect_err("--tanstack must reject a procedure colliding with a model's generated hook");

    match error {
        TypeScriptGeneratorError::TanstackHookNameCollision {
            procedure,
            identifier,
            model,
            operation,
        } => {
            assert_eq!(procedure, expected_procedure);
            assert_eq!(identifier, expected_identifier);
            assert_eq!(model, "Post");
            assert_eq!(operation, expected_operation);
        }
        other => panic!("expected TanstackHookNameCollision, got {other:?}"),
    }
}

#[test]
fn tanstack_rejects_a_mutation_procedure_colliding_with_a_model_hook() {
    expect_collision(
        MUTATION_FIXTURE,
        "create_post",
        "useCreatePostMutation",
        "create",
    );
}

#[test]
fn tanstack_rejects_a_query_procedure_colliding_with_a_model_hook() {
    expect_collision(QUERY_FIXTURE, "post_list", "usePostListQuery", "list");
}

/// The transport-parity half. A REST-only fix would pass every other test
/// here while leaving the identical hazard live in
/// `rpc-react-query.ts.j2`.
#[test]
fn tanstack_rejects_the_same_collision_on_rpc_transport() {
    let schema = parse(QUERY_FIXTURE_RPC);
    assert_eq!(
        schema.transport,
        cratestack_core::TransportStyle::Rpc,
        "fixture must actually be RPC transport, or this test silently \
         re-runs the REST case"
    );
    expect_collision(QUERY_FIXTURE_RPC, "post_list", "usePostListQuery", "list");
}

/// The check must fire **only** under `--tanstack` (decision spike #317).
/// Without this, rejecting unconditionally would satisfy every assertion
/// above while breaking schemas that never touch the flag.
///
/// `--swr` is included alongside the default layout deliberately: these
/// fixtures are built on `create_post`/`post_list`, and `--swr`'s own
/// #777 check compares `to_camel_case` forms (`createPost`, `postList`)
/// against model *function* names — `createPost` genuinely collides
/// there, so only the query fixture is a valid `--swr` control. Asserting
/// the wrong one would have been a test that passes for the wrong reason.
#[test]
fn default_and_swr_layouts_accept_what_tanstack_rejects() {
    for fixture_path in [MUTATION_FIXTURE, QUERY_FIXTURE, QUERY_FIXTURE_RPC] {
        let schema = parse(fixture_path);
        generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                package_name: "default-fixture-client".to_owned(),
                ..TypeScriptGeneratorConfig::default()
            },
        )
        .unwrap_or_else(|error| {
            panic!("default layout must not be constrained by --tanstack's naming scheme, but {fixture_path} failed: {error}")
        });
    }

    // `--swr` on the query fixture only — see this test's doc comment for
    // why the mutation fixture is not a valid control here.
    let schema = parse(QUERY_FIXTURE);
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--swr must not be constrained by --tanstack's naming scheme");
}
