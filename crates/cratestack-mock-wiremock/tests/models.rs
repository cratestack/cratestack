//! `transport rest` model CRUD stubs are now stateful (`wiremock-state-
//! extension`-backed) — these tests assert on the *shape* of the
//! generated Handlebars template strings and `serveEventListeners`,
//! since the actual create-then-list/update-then-get/delete-then-404
//! behavior can only be proven against a real WireMock instance with
//! the extension loaded (see `docs/design/wiremock-stubs.md`'s "Model
//! CRUD statefulness" section for that evidence). `transport rpc` model
//! CRUD stays static/deterministic — see `mappings_are_static_not_stateful_under_rpc_transport`.

use cratestack_mock_wiremock::{WireMockGeneratorConfig, WireMockGeneratorError, generate_package};

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

const PG_DATASOURCE: &str = "datasource db {
  provider = \"postgresql\"
  url = env(\"DATABASE_URL\")
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
fn generates_five_mapping_files_per_model_in_alphabetical_order() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();

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
}

#[test]
fn rest_list_and_create_share_the_plural_collection_path() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let list = mapping(&package, "mappings/model.Widget.list.json");
    assert_eq!(list["request"]["method"], "GET");
    assert_eq!(list["request"]["urlPath"], "/api/widgets");
    assert_eq!(list["response"]["status"], 200);
    assert_eq!(list["metadata"]["cratestack"]["stateful"], true);

    let create = mapping(&package, "mappings/model.Widget.create.json");
    assert_eq!(create["request"]["method"], "POST");
    assert_eq!(create["request"]["urlPath"], "/api/widgets");
    // `StatusCode::CREATED`, not `OK` — the one place a model stub's status
    // differs from every procedure stub's literal `200`
    // (`crates/cratestack-macros/src/axum/model/handlers_crud.rs`'s
    // `build_create_handler`).
    assert_eq!(create["response"]["status"], 201);
    let create_body = body(&create);
    assert!(create_body.contains("\"id\": 1{{randomValue length=5 type='NUMERIC'}}"));
    assert!(create_body.contains("(jsonPath request.body '$.name')"));
}

#[test]
fn rest_get_update_delete_use_a_state_matcher_pattern_route_and_priority_one() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    for (verb, method, status) in [
        ("get", "GET", 200),
        ("update", "PATCH", 200),
        ("delete", "DELETE", 200),
    ] {
        let mapping = mapping(&package, &format!("mappings/model.Widget.{verb}.json"));
        assert_eq!(mapping["request"]["method"], method, "{verb} method");
        assert!(
            mapping["request"].get("urlPath").is_none(),
            "{verb} must not use an exact urlPath — there is no fixed id to match"
        );
        assert_eq!(
            mapping["request"]["urlPathPattern"], "^/api/widgets/[^/]+$",
            "{verb} urlPathPattern"
        );
        assert_eq!(
            mapping["request"]["customMatcher"]["name"], "state-matcher",
            "{verb} customMatcher"
        );
        assert_eq!(
            mapping["request"]["customMatcher"]["parameters"]["hasContext"], "{{request.path}}",
            "{verb} hasContext — must gate on THIS request's own detail-route path"
        );
        assert_eq!(mapping["priority"], 1, "{verb} priority");
        assert_eq!(mapping["response"]["status"], status, "{verb} status");
    }

    let get_body = body(&mapping(&package, "mappings/model.Widget.get.json"));
    assert!(get_body.contains("{{state context=request.path property='name'}}"));

    let update_body = body(&mapping(&package, "mappings/model.Widget.update.json"));
    assert!(update_body.contains("(jsonPath request.body '$.name')"));
    assert!(
        update_body.contains("{{state context=request.path property='name'}}"),
        "a PATCH that omits `name` must fall back to the prior stored value: {update_body}"
    );

    let delete_body = body(&mapping(&package, "mappings/model.Widget.delete.json"));
    assert_eq!(
        delete_body, get_body,
        "delete's response is the pre-delete snapshot — same read as get"
    );
}

#[test]
fn create_update_delete_persist_through_serve_event_listeners() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let create = mapping(&package, "mappings/model.Widget.create.json");
    let create_listeners = create["serveEventListeners"].as_array().unwrap();
    assert_eq!(create_listeners.len(), 2, "list-append + per-record state");
    assert_eq!(create_listeners[0]["parameters"]["context"], "widgets");
    assert!(
        create_listeners[1]["parameters"]["context"]
            .as_str()
            .unwrap()
            .starts_with("/api/widgets/"),
        "the per-record context must be keyed off the real detail route: {create_listeners:?}"
    );

    let update = mapping(&package, "mappings/model.Widget.update.json");
    let update_listeners = update["serveEventListeners"].as_array().unwrap();
    assert_eq!(
        update_listeners.len(),
        3,
        "overwrite per-record state, remove stale list entry, re-add updated one"
    );
    assert_eq!(update_listeners[0]["name"], "recordState");
    assert_eq!(update_listeners[1]["name"], "deleteState");
    assert_eq!(update_listeners[2]["name"], "recordState");

    let delete = mapping(&package, "mappings/model.Widget.delete.json");
    let delete_listeners = delete["serveEventListeners"].as_array().unwrap();
    assert_eq!(
        delete_listeners.len(),
        2,
        "drop per-record state + list entry"
    );
    assert_eq!(delete_listeners[0]["name"], "deleteState");
    assert_eq!(delete_listeners[1]["name"], "deleteState");
}

#[test]
fn paged_model_list_uses_the_items_total_count_page_info_envelope() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Post {{
  id Int @id
  title String

  @@paged
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let list_body = body(&mapping(&package, "mappings/model.Post.list.json"));

    assert!(list_body.contains("\"items\": ["));
    assert!(
        list_body.contains(
            "\"totalCount\": {{size (state context='posts' property='list' default='[]')}}"
        )
    );
    assert!(list_body.contains("\"hasNextPage\": false"));
    assert!(list_body.contains("\"hasPreviousPage\": false"));
}

#[test]
fn non_paged_model_list_is_a_bare_array_not_an_envelope() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let list_body = body(&mapping(&package, "mappings/model.Widget.list.json"));

    assert!(list_body.trim_start().starts_with('['));
    assert!(!list_body.contains("\"items\""));
}

#[test]
fn relation_fields_are_excluded_from_the_synthesized_record() {
    // Mirrors `docs/design/wiremock-stubs.md`'s real fixture shape
    // (`ci_rest.cstack`): a relation is only populated via
    // `include=<relation>`, so the default projection this generator
    // mirrors must not include it either.
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Author {{
  id Int @id
  name String
  posts Post[] @relation(fields:[id],references:[authorId])
}}

model Post {{
  id Int @id
  title String
  authorId Int
  author Author @relation(fields:[authorId],references:[id])
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let author_body = body(&mapping(&package, "mappings/model.Author.get.json"));
    assert!(author_body.contains("\"name\""));
    assert!(
        !author_body.contains("\"posts\""),
        "relation field `posts` must not appear: {author_body}"
    );

    let post_body = body(&mapping(&package, "mappings/model.Post.get.json"));
    assert!(post_body.contains("\"authorId\""));
    assert!(
        !post_body.contains("\"author\":"),
        "relation field `author` must not appear: {post_body}"
    );
}

#[test]
fn server_only_fields_are_excluded_from_the_synthesized_record() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
  internalNotes String @server_only
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let get_body = body(&mapping(&package, "mappings/model.Widget.get.json"));

    assert!(get_body.contains("\"name\""));
    assert!(
        !get_body.contains("internalNotes"),
        "@server_only field must never reach a client-facing body: {get_body}"
    );
}

#[test]
fn unsupported_field_types_fall_back_to_a_frozen_static_value_not_a_template() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
  nickname String?
  metadata Json
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    for verb in ["create", "get", "update", "delete"] {
        let body = body(&mapping(
            &package,
            &format!("mappings/model.Widget.{verb}.json"),
        ));
        assert!(
            body.contains("\"nickname\": \"string\""),
            "{verb}: Optional field should be a frozen literal: {body}"
        );
        assert!(
            body.contains("\"metadata\": {}"),
            "{verb}: Json field should be a frozen literal: {body}"
        );
        // The stateful `name` field IS templated — contrast confirms the
        // frozen fields above are frozen on purpose, not because nothing
        // in this body is ever templated.
        assert!(
            body.contains("{{"),
            "{verb}: the model's own `name` field should still be templated: {body}"
        );
    }
}

#[test]
fn mappings_are_static_not_stateful_under_rpc_transport() {
    let schema = schema(&format!(
        "transport rpc

{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String
}}
"
    ));

    let package = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    let expectations = [
        ("list", "GET"),
        ("get", "GET"),
        ("create", "POST"),
        ("update", "PATCH"),
        ("delete", "DELETE"),
    ];
    for (verb, _rest_method_that_must_not_appear) in expectations {
        let mapping = mapping(&package, &format!("mappings/model.Widget.{verb}.json"));
        // Every RPC model route is POST, regardless of the REST verb it
        // stands in for (`generate_model_rpc_dispatch_arms` in
        // `crates/cratestack-macros/src/transport/rpc.rs`).
        assert_eq!(mapping["request"]["method"], "POST", "{verb} method");
        assert_eq!(
            mapping["request"]["urlPath"],
            format!("/api/rpc/model.Widget.{verb}"),
            "{verb} urlPath"
        );
        assert!(
            mapping["request"].get("customMatcher").is_none(),
            "{verb}: RPC model routes stay static — no state-matcher gating"
        );
        assert!(
            mapping["serveEventListeners"].is_null(),
            "{verb}: RPC model routes stay static — nothing persisted"
        );
        assert!(
            mapping["metadata"]["cratestack"].get("stateful").is_none(),
            "{verb}: static mappings must not claim to be stateful"
        );
        // A real `jsonBody`, not a Handlebars template string — proves
        // this is the frozen v1 shape, not the stateful REST one.
        assert!(
            mapping["response"]["jsonBody"].is_object()
                || mapping["response"]["jsonBody"].is_array()
        );
    }

    assert_eq!(
        mapping(&package, "mappings/model.Widget.create.json")["response"]["status"],
        201
    );
}

#[test]
fn custom_base_path_with_a_regex_metacharacter_is_escaped_in_the_detail_pattern() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
}}
"
    ));

    let config = WireMockGeneratorConfig {
        base_path: "/api.v2".to_owned(),
    };
    let package = generate_package(&schema, &config).unwrap();
    let get = mapping(&package, "mappings/model.Widget.get.json");

    // A literal, un-escaped `.` in a `urlPathPattern` regex would also
    // match `/api-v2/widgets/1` — not what `--base-path /api.v2` means.
    assert_eq!(
        get["request"]["urlPathPattern"],
        "^/api\\.v2/widgets/[^/]+$"
    );
}

#[test]
fn model_missing_a_primary_key_is_a_hard_error_not_a_panic() {
    // Schema validation already requires every REST/RPC model to declare
    // an `@id` field; `parse_schema_unvalidated` bypasses that check the
    // same way a hand-built or pre-validation-rule `Schema` value could,
    // so this exercises the defense-in-depth path directly (see
    // `WireMockGeneratorError::ModelMissingPrimaryKey`'s own doc comment).
    let schema = cratestack_parser::parse_schema_unvalidated(&format!(
        "{PG_DATASOURCE}
model Widget {{
  name String
}}
"
    ))
    .expect("unvalidated parse should still succeed");

    let error = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect_err("a model with no @id field cannot have get/update/delete routes derived");
    assert!(matches!(
        error,
        WireMockGeneratorError::ModelMissingPrimaryKey { model } if model == "Widget"
    ));
}

#[test]
fn composite_primary_key_is_rejected_with_the_shared_cratestack_core_message() {
    // Same guard, same message, as `generate-typescript`/`generate-dart`
    // (cratestack#590) — see `cratestack_core::composite_id`.
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model AccountMembership {{
  accountId Int
  subject String
  @@id([accountId, subject])
}}
"
    ));

    let error = generate_package(&schema, &WireMockGeneratorConfig::default())
        .expect_err("a composite @@id model must be rejected, not generated");
    let WireMockGeneratorError::CompositePrimaryKeyUnsupported(message) = &error else {
        panic!("expected CompositePrimaryKeyUnsupported, got: {error}");
    };
    assert!(
        message.contains("AccountMembership") && message.contains("issues/136"),
        "message should name the model and the tracking issue: {message}"
    );
}

#[test]
fn same_schema_generates_byte_identical_model_mappings_twice() {
    let schema = schema(&format!(
        "{PG_DATASOURCE}
model Widget {{
  id Int @id
  name String

  @@paged
}}
"
    ));

    let first = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();
    let second = generate_package(&schema, &WireMockGeneratorConfig::default()).unwrap();

    assert_eq!(
        first, second,
        "generation must be deterministic for --check to be a meaningful gate — the Handlebars \
         TEXT is fixed even though what it renders to at request time isn't"
    );
}
