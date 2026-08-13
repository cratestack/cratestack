//! Offline proof (no Docker, no network) that `schema.cstack` parses and
//! that both generators this example depends on — `generate-typescript
//! --refine` and `generate-wiremock` — produce the shape `web/` and
//! `README.md` assume. The real end-to-end run (a live WireMock
//! container, real HTTP CRUD, the Vite app) is documented and was
//! performed manually — see `README.md`'s "Verification" section; CI's
//! `js (react-vite-refine example)` job re-runs the live-container half
//! on every PR (see `.github/workflows/ci.yml`).

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package as generate_ts};
use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package as generate_wiremock};
use react_vite_refine_example::SCHEMA_PATH;

fn schema() -> cratestack_core::Schema {
    cratestack_parser::parse_schema_file(SCHEMA_PATH).expect("schema.cstack should parse")
}

#[test]
fn schema_declares_the_three_intended_models() {
    let schema = schema();
    let names: Vec<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Category", "Post", "Tag"], "{names:?}");
    assert_eq!(schema.transport, cratestack_core::TransportStyle::Rest);
}

#[test]
fn typescript_client_emits_a_refine_manifest_with_the_three_distinct_facts() {
    let schema = schema();
    let config = TypeScriptGeneratorConfig {
        package_name: "react-vite-refine-client".to_owned(),
        refine: true,
        ..Default::default()
    };
    let package = generate_ts(&schema, &config).expect("generate-typescript --refine");

    let refine_ts = package
        .files
        .iter()
        .find(|f| f.file_name == "src/refine.ts")
        .expect("--refine must emit src/refine.ts")
        .contents
        .clone();

    // Category: the plain case — no paged, no versionField.
    assert!(refine_ts.contains(r#""categories": {"#));
    assert!(refine_ts.contains(r#"primaryKey: "id","#) && refine_ts.contains("categories"));
    // Post: @@paged + @version.
    assert!(refine_ts.contains(r#""posts": {"#));
    assert!(refine_ts.contains("paged: true,"));
    assert!(refine_ts.contains(r#"versionField: "version","#));
    // Tag: @id not named `id`.
    assert!(refine_ts.contains(r#""tags": {"#));
    assert!(refine_ts.contains(r#"primaryKey: "slug","#));
}

#[test]
fn wiremock_generates_five_stateful_mappings_per_model() {
    let schema = schema();
    let package =
        generate_wiremock(&schema, &WireMockGeneratorConfig::default()).expect("generate-wiremock");

    let names: Vec<&str> = package.files.iter().map(|f| f.file_name.as_str()).collect();
    for model in ["Category", "Post", "Tag"] {
        for verb in ["create", "delete", "get", "list", "update"] {
            let expected = format!("mappings/model.{model}.{verb}.json");
            assert!(
                names.contains(&expected.as_str()),
                "missing {expected} in {names:?}"
            );
        }
    }
}

#[test]
fn post_create_and_update_round_trip_the_falsy_published_field() {
    // cratestack#588: a falsy value (`false`) must be distinguishable from
    // "field omitted" in the generated Handlebars template, not silently
    // coerced to the field's static default. `published Boolean` on `Post`
    // is this example's demonstration of that fix — assert the generated
    // template still uses presence-testing (`default='...'` + `eq`), not
    // truthiness-testing, so a regression here is caught before it ships.
    let schema = schema();
    let package =
        generate_wiremock(&schema, &WireMockGeneratorConfig::default()).expect("generate-wiremock");
    let create = package
        .files
        .iter()
        .find(|f| f.file_name == "mappings/model.Post.create.json")
        .unwrap();
    let mapping: serde_json::Value = serde_json::from_str(&create.contents).unwrap();
    let body = mapping["response"]["body"].as_str().unwrap();
    assert!(body.contains("(jsonPath request.body '$.published' default="));
    assert!(body.contains("(eq (jsonPath request.body '$.published'"));
}

#[test]
fn wiremock_stubs_enforce_if_match_on_the_versioned_model() {
    // The inverse of what this file asserted until #605. `Post` declares
    // `@version`, so `cratestack-mock-wiremock` now emits real
    // header-precondition matching for its `update`/`delete`: a quoted
    // integer `If-Match` compared against the record's CURRENT stored
    // version, via `state-matcher`'s templated `property` check.
    //
    // This was previously a trip-wire asserting the ABSENCE of header
    // matching, with a failure message telling whoever broke it to update
    // the README. It broke, on purpose, when #605 landed — which is how
    // the docs got corrected instead of silently going stale. Kept
    // inverted rather than deleted: it now guards the capability rather
    // than the gap.
    let schema = schema();
    let package =
        generate_wiremock(&schema, &WireMockGeneratorConfig::default()).expect("generate-wiremock");
    for verb in ["update", "delete"] {
        let stubs: Vec<serde_json::Value> = package
            .files
            .iter()
            .filter(|f| {
                f.file_name
                    .starts_with(&format!("mappings/model.Post.{verb}"))
            })
            .map(|f| serde_json::from_str(&f.contents).expect("stub is valid json"))
            .collect();
        assert!(
            !stubs.is_empty(),
            "{verb}: no Post stubs emitted at all — the file naming changed"
        );
        assert!(
            stubs.iter().any(|m| m["request"].get("headers").is_some()),
            "{verb}: no stub matches on request headers — If-Match enforcement regressed: {stubs:?}"
        );
        // The success path must compare against STORED state, not a
        // constant: a matcher that accepts any well-formed ETag would
        // pass a naive "has headers" check while enforcing nothing.
        assert!(
            stubs.iter().any(|m| {
                m["request"]["customMatcher"]["parameters"]
                    .get("property")
                    .is_some()
            }),
            "{verb}: no stub compares If-Match against stored version state: {stubs:?}"
        );
    }
}

/// A versioned model's mutation responses must carry the NEW version as a
/// quoted-integer `ETag`, or a client cannot round-trip `GET` -> `PATCH`.
#[test]
fn versioned_update_returns_a_bumped_etag() {
    let schema = schema();
    let package =
        generate_wiremock(&schema, &WireMockGeneratorConfig::default()).expect("generate-wiremock");
    let bumped = package
        .files
        .iter()
        .filter(|f| f.file_name.starts_with("mappings/model.Post.update"))
        .any(|f| f.contents.contains("ETag") && f.contents.contains("'+' 1"));
    assert!(
        bumped,
        "no Post.update stub returns an incremented ETag; the version no longer bumps"
    );
}
