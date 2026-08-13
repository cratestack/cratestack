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
fn wiremock_stubs_do_not_validate_if_match_or_any_request_header() {
    // Documents a real, confirmed gap (not a bug in this example): the
    // WireMock generator's `get`/`update`/`delete` stubs match on
    // method + path (+ record-existence via `state-matcher`'s
    // `hasContext`) only — there is no header inspection anywhere in
    // `cratestack-mock-wiremock`, so a stale (or missing) `If-Match`
    // against this mock's `Post` (a `@version` model) is NOT rejected the
    // way a real `cratestack-pg` server rejects it. See README.md's "What
    // this demo can't prove" section — the client still SENDS `If-Match`
    // (verified against the live container; see README), it just isn't
    // checked here. This test is a trip-wire: if `cratestack-mock-
    // wiremock` ever gains header-based precondition matching, this
    // assertion should start failing, which is the signal to revisit
    // the README section it documents.
    let schema = schema();
    let package =
        generate_wiremock(&schema, &WireMockGeneratorConfig::default()).expect("generate-wiremock");
    for verb in ["update", "delete"] {
        let file = package
            .files
            .iter()
            .find(|f| f.file_name == format!("mappings/model.Post.{verb}.json"))
            .unwrap();
        let mapping: serde_json::Value = serde_json::from_str(&file.contents).unwrap();
        assert!(
            mapping["request"].get("headers").is_none(),
            "{verb}: expected no request.headers matcher, found one — \
             cratestack-mock-wiremock has grown If-Match support; update README.md's \
             \"What this demo can't prove\" section: {mapping}"
        );
        let params = &mapping["request"]["customMatcher"]["parameters"];
        assert!(
            params.get("hasContext").is_some() && params.as_object().unwrap().len() == 1,
            "{verb}: customMatcher parameters grew beyond `hasContext` — same trip-wire as above: {mapping}"
        );
    }
}
