//! cratestack#1037 (MCP phase 2): the generated JSON Schemas for `model`s
//! in procedure I/O agree with serde's wire shape for the real generated
//! model struct. The scalar, list, enum and `decimal = BigDecimal`
//! coverage lives in `cratestack-api`'s `tests/json_schema_*.rs`; this
//! suite needs `db = Postgres`, the only mode with models. No database is
//! touched: everything here is serde and schema validation.

use cratestack::include_server_schema;
use jsonschema::Validator;
use serde_json::{Value, json};

include_server_schema!(
    "tests/fixtures/json_schema_models.cstack",
    db = Postgres,
    decimal = RustDecimal
);

const SCHEMAS: &[(&str, Result<&str, &str>, Result<Option<&str>, &str>)] = cratestack_macros::__procedure_json_schemas!(
    "tests/fixtures/json_schema_models.cstack",
    decimal = RustDecimal
);

use cratestack_schema::procedures::{feed, fetch_post, import_post};
use cratestack_schema::{Post, Visibility};

fn schema(name: &str, output: bool) -> Value {
    let (_, input_schema, output_schema) = SCHEMAS.iter().find(|(n, _, _)| *n == name).unwrap();
    let text = if output {
        output_schema.unwrap().expect("an object output")
    } else {
        input_schema.unwrap()
    };
    serde_json::from_str(text).unwrap()
}

fn validator(name: &str, output: bool) -> Validator {
    let schema = schema(name, output);
    jsonschema::draft202012::meta::validate(&schema).expect("valid 2020-12");
    jsonschema::draft202012::new(&schema).expect("compiles")
}

fn posts() -> Vec<Post> {
    let at = cratestack::chrono::DateTime::from_timestamp(1_700_000_000, 5).unwrap();
    let post = |id, body: Option<&str>, visibility, cover: Option<Vec<u8>>| Post {
        id,
        title: format!("post {id}"),
        body: body.map(str::to_owned),
        price: "12.50".parse().unwrap(),
        visibility,
        secret: "hunter2".to_owned(),
        authorId: 7,
        publishedAt: cover.as_ref().map(|_| at),
        cover,
    };
    vec![
        post(1, None, Visibility::Public, None),
        post(
            i64::MAX,
            Some("body"),
            Visibility::Private,
            Some(vec![0, 255]),
        ),
    ]
}

fn assert_valid(validator: &Validator, instance: &Value, context: &str) {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| e.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "{context}: rejected {instance}: {errors:#?}"
    );
}

#[test]
fn a_model_round_trips_as_output_argument_and_page_item() {
    let (get, import, feed_output) = (
        validator("fetchPost", true),
        validator("importPost", false),
        validator("feed", true),
    );
    for post in posts() {
        let written: fetch_post::Output = post.clone();
        let written = serde_json::to_value(&written).unwrap();
        assert!(
            written.get("secret").is_none(),
            "`@server_only` is skip_serializing"
        );
        assert_valid(&get, &written, "fetchPost output");
        let args = serde_json::to_value(import_post::Args { post }).unwrap();
        assert_valid(&import, &args, "importPost input");
    }
    let page: feed::Output = cratestack::Page::new(posts(), Default::default());
    assert_valid(
        &feed_output,
        &serde_json::to_value(&page).unwrap(),
        "feed output",
    );
}

#[test]
fn relations_and_server_only_fields_are_not_advertised() {
    let post = &schema("fetchPost", true)["$defs"]["Post"];
    let mut properties: Vec<&str> = post["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    properties.sort_unstable();
    let expected = [
        "authorId",
        "body",
        "cover",
        "id",
        "price",
        "publishedAt",
        "title",
        "visibility",
    ];
    assert_eq!(properties, expected);
    assert_eq!(
        post["required"],
        json!(["id", "title", "price", "visibility", "authorId"])
    );
    assert_eq!(post["description"], "A published piece.");
}

#[test]
fn a_model_argument_rejects_wrong_shapes() {
    let import = validator("importPost", false);
    let base = serde_json::to_value(&posts()[1]).unwrap();
    for (field, value) in [
        ("title", None),
        ("visibility", None),
        ("price", Some(json!(12.5))),
        ("visibility", Some(json!("Hidden"))),
        ("body", Some(json!(5))),
        ("cover", Some(json!("AAE="))),
        ("authorId", Some(json!("7"))),
        ("publishedAt", Some(json!("2024-01-01"))),
    ] {
        let mut post = base.clone();
        let object = post.as_object_mut().unwrap();
        match &value {
            Some(value) => object.insert(field.to_owned(), value.clone()),
            None => object.remove(field),
        };
        let args = json!({ "post": post });
        let context = format!("importPost `{field}` = {value:?}");
        assert!(!import.is_valid(&args), "{context}: schema accepted it");
        let decoded = serde_json::from_value::<import_post::Args>(args);
        assert!(decoded.is_err(), "{context}: serde accepted it");
    }
}

/// A `@server_only` field is left out of the schema so its name is never
/// advertised, and since #1057 serde skips it on input too, so schema and
/// serde agree: a client-sent value, of any type, is accepted and dropped.
/// (Before #1057 serde still parsed the key, so a wrong-typed value failed
/// deserialization while the schema let it through — the gap this test
/// used to pin.)
#[test]
fn a_server_only_field_on_input_is_ignored_by_schema_and_serde_alike() {
    let import = validator("importPost", false);
    for sent in [json!(5), json!("from-agent")] {
        let mut post = serde_json::to_value(&posts()[0]).unwrap();
        post.as_object_mut()
            .unwrap()
            .insert("secret".to_owned(), sent.clone());
        let args = json!({ "post": post });
        assert!(import.is_valid(&args), "schema rejected `secret` = {sent}");
        let decoded = serde_json::from_value::<import_post::Args>(args)
            .unwrap_or_else(|error| panic!("serde rejected `secret` = {sent}: {error}"));
        assert_eq!(
            decoded.post.secret,
            String::default(),
            "a client-sent `secret` = {sent} reached the procedure"
        );
    }
}
