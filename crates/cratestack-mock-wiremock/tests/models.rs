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
    assert_eq!(
        list["response"]["jsonBody"],
        serde_json::json!([{ "id": 0, "name": "string" }])
    );

    let create = mapping(&package, "mappings/model.Widget.create.json");
    assert_eq!(create["request"]["method"], "POST");
    assert_eq!(create["request"]["urlPath"], "/api/widgets");
    // `StatusCode::CREATED`, not `OK` — the one place a model stub's status
    // differs from every procedure stub's literal `200`
    // (`crates/cratestack-macros/src/axum/model/handlers_crud.rs`'s
    // `build_create_handler`).
    assert_eq!(create["response"]["status"], 201);
    assert_eq!(
        create["response"]["jsonBody"],
        serde_json::json!({ "id": 0, "name": "string" })
    );
}

#[test]
fn rest_get_update_delete_share_a_detail_path_pattern_not_an_exact_path() {
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
        assert_eq!(mapping["response"]["status"], status, "{verb} status");
        assert_eq!(
            mapping["response"]["jsonBody"],
            serde_json::json!({ "id": 0, "name": "string" }),
            "{verb} body"
        );
    }
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
    let list = mapping(&package, "mappings/model.Post.list.json");
    let body = &list["response"]["jsonBody"];

    assert_eq!(
        body["items"],
        serde_json::json!([{ "id": 0, "title": "string" }])
    );
    assert_eq!(body["totalCount"], 1);
    assert_eq!(body["pageInfo"]["hasNextPage"], false);
    assert_eq!(body["pageInfo"]["hasPreviousPage"], false);
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
    let list = mapping(&package, "mappings/model.Widget.list.json");

    assert!(list["response"]["jsonBody"].is_array());
    assert!(list["response"]["jsonBody"]["items"].is_null());
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

    let author_get = mapping(&package, "mappings/model.Author.get.json");
    let author_body = &author_get["response"]["jsonBody"];
    assert_eq!(author_body["id"], 0);
    assert_eq!(author_body["name"], "string");
    assert!(
        author_body.get("posts").is_none(),
        "relation field `posts` must not appear in the default-projection record: {author_body}"
    );

    let post_get = mapping(&package, "mappings/model.Post.get.json");
    let post_body = &post_get["response"]["jsonBody"];
    assert_eq!(post_body["id"], 0);
    assert_eq!(post_body["title"], "string");
    assert_eq!(post_body["authorId"], 0);
    assert!(
        post_body.get("author").is_none(),
        "relation field `author` must not appear in the default-projection record: {post_body}"
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
    let body = &mapping(&package, "mappings/model.Widget.get.json")["response"]["jsonBody"];

    assert_eq!(body["name"], "string");
    assert!(
        body.get("internalNotes").is_none(),
        "@server_only field must never reach a client-facing body: {body}"
    );
}

#[test]
fn rpc_transport_model_routes_use_the_model_dot_name_dot_verb_op_id() {
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
            mapping["request"].get("urlPathPattern").is_none(),
            "RPC routes are always exact — the id lives in the body, not the URL"
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
        "generation must be deterministic for --check to be a meaningful gate"
    );
}
