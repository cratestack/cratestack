//! `@version` model coverage for `transport rest`'s stateful CRUD
//! generator — the `If-Match` gating this generator previously never
//! emitted at all (every request-header match a model stub can carry
//! comes from here). Like `tests/models.rs`, these assert on the
//! *shape* of the generated stubs; the live create/patch/delete round
//! trip against a real `wiremock-state-extension` container is verified
//! by hand and documented in `docs/design/wiremock-stubs.md`'s
//! "If-Match / optimistic locking" section, since CI has no Docker
//! build step for this crate.

use cratestack_mock_wiremock::{WireMockGeneratorConfig, generate_package};

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

const PG_DATASOURCE: &str = "datasource db {
  provider = \"postgresql\"
  url = env(\"DATABASE_URL\")
}
";

const VERSIONED_WIDGET: &str = "model Widget {
  id Int @id
  name String
  version Int @version
}
";

fn mapping(
    package: &cratestack_mock_wiremock::GeneratedWireMockPackage,
    file_name: &str,
) -> serde_json::Value {
    let file = package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| {
            panic!(
                "no generated file named {file_name} (have: {:?})",
                package
                    .files
                    .iter()
                    .map(|f| &f.file_name)
                    .collect::<Vec<_>>()
            )
        });
    serde_json::from_str(&file.contents).expect("generated file should be valid JSON")
}

fn body(mapping: &serde_json::Value) -> String {
    mapping["response"]["body"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("expected a raw templated `response.body` string, got: {mapping}")
        })
        .to_owned()
}

#[test]
fn versioned_model_fans_update_and_delete_into_five_if_match_gated_stubs_each() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "mappings/model.Widget.create.json",
            "mappings/model.Widget.delete-if-match-malformed.json",
            "mappings/model.Widget.delete-if-match-required.json",
            "mappings/model.Widget.delete-if-match-stale.json",
            "mappings/model.Widget.delete-if-match-wildcard.json",
            "mappings/model.Widget.delete.json",
            "mappings/model.Widget.get.json",
            "mappings/model.Widget.list.json",
            "mappings/model.Widget.update-if-match-malformed.json",
            "mappings/model.Widget.update-if-match-required.json",
            "mappings/model.Widget.update-if-match-stale.json",
            "mappings/model.Widget.update-if-match-wildcard.json",
            "mappings/model.Widget.update.json",
        ],
        "an @version model's update/delete must each fan out into 5 stubs; \
         list/get/create must stay one file each"
    );
}

#[test]
fn versioned_model_if_match_stubs_have_the_right_status_headers_and_priority_per_case() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    // (file suffix, expected status, expected request "headers" matcher,
    // expected priority) — the exact contract table this generator must
    // mirror from `parse_if_match_version` + `CoolError::status_code`.
    let cases = [
        (
            "if-match-required",
            412,
            serde_json::json!({ "If-Match": { "absent": true } }),
            1,
        ),
        (
            "if-match-wildcard",
            400,
            serde_json::json!({ "If-Match": { "equalTo": "*" } }),
            2,
        ),
        (
            "if-match-malformed",
            400,
            serde_json::json!({ "If-Match": { "doesNotMatch": "^\"-?[0-9]+\"$" } }),
            3,
        ),
        (
            "if-match-stale",
            412,
            serde_json::json!({ "If-Match": { "matches": "^\"-?[0-9]+\"$" } }),
            4,
        ),
    ];

    for verb in ["update", "delete"] {
        for (suffix, status, headers, priority) in &cases {
            let file = format!("mappings/model.Widget.{verb}-{suffix}.json");
            let m = mapping(&package, &file);
            assert_eq!(m["response"]["status"], *status, "{file} status");
            assert_eq!(m["request"]["headers"], *headers, "{file} request headers");
            assert_eq!(m["priority"], *priority, "{file} priority");
            assert_eq!(
                m["request"]["customMatcher"]["parameters"]["hasContext"], "{{request.path}}",
                "{file} must still gate on record existence"
            );
        }

        // The success stub (no suffix) is the 5th, lowest-priority case,
        // gated on a well-formed AND *current* version.
        let success = mapping(&package, &format!("mappings/model.Widget.{verb}.json"));
        assert_eq!(success["priority"], 5, "{verb} success priority");
        assert_eq!(
            success["request"]["headers"],
            serde_json::json!({ "If-Match": { "matches": "^\"-?[0-9]+\"$" } }),
            "{verb} success request headers"
        );
        assert_eq!(
            success["request"]["customMatcher"]["parameters"]["property"]["version"]["equalTo"],
            "{{regexExtract request.headers.If-Match '[0-9]+' default='__cratestack_if_match_no_digits__'}}",
            "{verb} success must compare the stored version against the header's digits"
        );

        let stale = mapping(
            &package,
            &format!("mappings/model.Widget.{verb}-if-match-stale.json"),
        );
        assert_eq!(
            stale["request"]["customMatcher"]["parameters"]["property"]["version"]["not"]["equalTo"],
            "{{regexExtract request.headers.If-Match '[0-9]+' default='__cratestack_if_match_no_digits__'}}",
            "{verb} stale case must be the exact negation of the success comparison"
        );
    }
}

#[test]
fn versioned_model_get_and_update_responses_carry_an_etag_delete_does_not() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let get = mapping(&package, "mappings/model.Widget.get.json");
    assert_eq!(
        get["response"]["headers"]["ETag"], "\"{{state context=request.path property='version'}}\"",
        "get must round-trip the CURRENT stored version as a strong ETag"
    );

    let update = mapping(&package, "mappings/model.Widget.update.json");
    assert_eq!(
        update["response"]["headers"]["ETag"],
        "\"{{math (state context=request.path property='version') '+' 1}}\"",
        "update's success response must carry the POST-BUMP version, not the pre-update one"
    );

    let delete = mapping(&package, "mappings/model.Widget.delete.json");
    assert!(
        delete["response"]["headers"].get("ETag").is_none(),
        "delete must never carry an ETag — the real server doesn't either \
         (no `delete_etag_apply` token exists, only `delete_if_match_apply`): {delete}"
    );

    let create = mapping(&package, "mappings/model.Widget.create.json");
    assert!(
        create["response"]["headers"].get("ETag").is_none(),
        "create must never carry an ETag either: {create}"
    );
}

#[test]
fn versioned_model_create_seeds_version_at_zero_never_from_the_request_body() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let create_body = body(&mapping(&package, "mappings/model.Widget.create.json"));
    assert!(
        create_body.contains("\"version\": 0"),
        "a fresh record's version must always start at the literal 0, mirroring \
         create_exec.rs's server-side seed: {create_body}"
    );
    assert!(
        !create_body.contains("jsonPath request.body '$.version'"),
        "version must never be merged/echoed from the client's create body — a real \
         Create<M>Input never carries @version: {create_body}"
    );
}

#[test]
fn versioned_model_update_success_bumps_the_stored_version_not_the_client_input() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let update_body = body(&mapping(&package, "mappings/model.Widget.update.json"));
    assert!(
        update_body.contains(
            "\"version\": {{math (state context=request.path property='version') '+' 1}}"
        ),
        "the success response must render the stored version plus one, mirroring \
         update_exec.rs's `version = version + 1`: {update_body}"
    );
    assert!(
        !update_body.contains("jsonPath request.body '$.version'"),
        "version must never be merged from the client's patch body either: {update_body}"
    );
}

#[test]
fn versioned_model_update_persists_the_bumped_version_harvested_from_the_response() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let update = mapping(&package, "mappings/model.Widget.update.json");
    // The success stub only — must be well-formed-AND-current gated, not
    // just present (a plain `hasContext`-only "update.json" also exists
    // pre-fix, so gating is the part that actually distinguishes this).
    assert_eq!(
        update["request"]["customMatcher"]["parameters"]["property"]["version"]["equalTo"],
        "{{regexExtract request.headers.If-Match '[0-9]+' default='__cratestack_if_match_no_digits__'}}"
    );
    let listeners = update["serveEventListeners"].as_array().unwrap();
    assert_eq!(listeners.len(), 1);
    assert_eq!(
        listeners[0]["parameters"]["state"]["version"], "{{jsonPath response.body '$.version'}}",
        "the bumped version must be harvested from the already-rendered response, \
         never recomputed a second time in the listener: {listeners:?}"
    );
}

#[test]
fn non_versioned_model_emits_no_header_matcher_anywhere() {
    // The main regression risk this change carries: a model with no
    // `@version` field must be byte-for-byte what the generator produced
    // before `If-Match` support existed — same 5 files, same
    // hasContext-only single stub per verb, same priority 1, no
    // `request.headers` key at all.
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
}}
"
    ));
    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "mappings/model.Widget.create.json",
            "mappings/model.Widget.delete.json",
            "mappings/model.Widget.get.json",
            "mappings/model.Widget.list.json",
            "mappings/model.Widget.update.json",
        ]
    );

    for verb in ["list", "get", "create", "update", "delete"] {
        let m = mapping(&package, &format!("mappings/model.Widget.{verb}.json"));
        assert!(
            m["request"].get("headers").is_none(),
            "{verb}: a non-versioned model must never get a request header matcher: {m}"
        );
        assert!(
            m["response"]["headers"].get("ETag").is_none(),
            "{verb}: a non-versioned model must never get an ETag response header: {m}"
        );
    }
    assert_eq!(
        mapping(&package, "mappings/model.Widget.update.json")["priority"],
        1
    );
    assert_eq!(
        mapping(&package, "mappings/model.Widget.delete.json")["priority"],
        1
    );
}

#[test]
fn versioned_model_generation_is_deterministic() {
    let schema = schema(&format!("{PG_DATASOURCE}\n{VERSIONED_WIDGET}"));

    let first = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let second = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    assert_eq!(
        first, second,
        "the 13-file fan-out for an @version model must stay deterministic, same as \
         the 5-file case — --check relies on this"
    );
}
