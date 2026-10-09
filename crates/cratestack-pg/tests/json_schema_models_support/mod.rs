//! ADR 0019: `BigInt` in model fields and procedure arguments, against the
//! generated MCP tool table. `reads` is a required `BigInt` model field,
//! `quota` an optional one, and `postsAbove(floor: BigInt, ids: BigInt[])`
//! takes `BigInt`s next to a model. Same contract as `cratestack-api`'s
//! `json_schema_support/bigint.rs`: a string with the canonical decimal
//! pattern, never an `integer`; `i64::MAX`, `i64::MIN` and `2^53 + 1`
//! validate as strings; a JSON number is refused by the schema and by serde;
//! the `i64` bound is the one place the schema is looser.

use cratestack::BigInt;
use serde_json::{Value, json};

use crate::cratestack_schema::Post;
use crate::cratestack_schema::procedures::{fetch_post, import_post, posts_above};
use crate::{assert_valid, posts, schema, validator};

/// ADR 0019's three pinned values, as the text the wire carries.
const PINNED: [(&str, i64); 3] = [
    ("9223372036854775807", i64::MAX),
    ("-9223372036854775808", i64::MIN),
    ("9007199254740993", 9_007_199_254_740_993),
];

fn fragment() -> Value {
    json!({ "type": "string", "pattern": "^(0|-?[1-9][0-9]{0,18})$" })
}

fn nullable() -> Value {
    json!({ "anyOf": [fragment(), { "type": "null" }] })
}

fn post_with(reads: i64, quota: Option<i64>) -> Post {
    let mut post = posts().remove(1);
    post.reads = BigInt::new(reads);
    post.quota = quota.map(BigInt::new);
    post
}

/// `post` with `field` replaced by `value` (removed when `None`).
fn edited(post: &Post, field: &str, value: Option<Value>) -> Value {
    let mut written = serde_json::to_value(post).unwrap();
    let object = written.as_object_mut().unwrap();
    match value {
        Some(value) => object.insert(field.to_owned(), value),
        None => object.remove(field),
    };
    written
}

#[test]
fn model_fields_and_arguments_are_canonical_decimal_strings() {
    let post = &schema("fetchPost", true)["$defs"]["Post"];
    assert_eq!(post["properties"]["reads"], fragment());
    assert_eq!(post["properties"]["quota"], nullable());
    let required = post["required"].as_array().unwrap();
    assert!(required.contains(&json!("reads")));
    assert!(!required.contains(&json!("quota")));

    let input = schema("postsAbove", false);
    assert_eq!(input["properties"]["floor"], fragment());
    assert_eq!(
        input["properties"]["ids"],
        json!({ "type": "array", "items": fragment() })
    );
    assert_eq!(input["required"], json!(["floor", "ids"]));
    assert!(
        !input.to_string().contains("integer"),
        "a `BigInt` argument is never an `integer`: {input}"
    );
    let page = schema("postsAbove", true);
    assert_eq!(page["$defs"]["Post"]["properties"]["reads"], fragment());
    assert_eq!(page["$defs"]["Post"]["properties"]["quota"], nullable());
}

#[test]
fn the_pinned_values_round_trip_through_model_argument_and_page_item() {
    let (fetch, import, above_in, above_out) = (
        validator("fetchPost", true),
        validator("importPost", false),
        validator("postsAbove", false),
        validator("postsAbove", true),
    );
    for (text, value) in PINNED {
        let post = post_with(value, Some(value));
        let written: fetch_post::Output = post.clone();
        let written = serde_json::to_value(&written).unwrap();
        assert_eq!(written["reads"], json!(text));
        assert_eq!(written["quota"], json!(text));
        assert_valid(&fetch, &written, text);
        let back: Post = serde_json::from_value(written).unwrap();
        assert_eq!(back.reads, BigInt::new(value), "{text}");

        let args = import_post::Args { post: post.clone() };
        assert_valid(&import, &serde_json::to_value(args).unwrap(), text);

        let args = posts_above::Args {
            floor: BigInt::new(value),
            ids: vec![BigInt::new(value), BigInt::new(0)],
        };
        let written = serde_json::to_value(&args).unwrap();
        assert_eq!(written, json!({ "floor": text, "ids": [text, "0"] }));
        assert_valid(&above_in, &written, text);

        let page: posts_above::Output = cratestack::Page::new(vec![post], Default::default());
        assert_valid(&above_out, &serde_json::to_value(&page).unwrap(), text);
    }
}

#[test]
fn a_json_number_is_refused_by_the_schema_and_by_serde() {
    let (fetch, import, above_in) = (
        validator("fetchPost", true),
        validator("importPost", false),
        validator("postsAbove", false),
    );
    let post = post_with(i64::MAX, Some(i64::MIN));
    let numbers = [
        json!(i64::MAX),
        json!(9_007_199_254_740_993_i64),
        json!(0),
        json!(1.5),
    ];
    for number in numbers {
        for field in ["reads", "quota"] {
            let context = format!("Post.{field} = {number}");
            let written = edited(&post, field, Some(number.clone()));
            assert!(!fetch.is_valid(&written), "{context}: output schema");
            let args = json!({ "post": written });
            assert!(!import.is_valid(&args), "{context}: input schema");
            assert!(
                serde_json::from_value::<import_post::Args>(args).is_err(),
                "{context}: serde accepted it"
            );
        }
        for (what, args) in [
            ("floor", json!({ "floor": number, "ids": [] })),
            ("ids[1]", json!({ "floor": "1", "ids": ["1", number] })),
        ] {
            let context = format!("postsAbove `{what}` = {number}");
            assert!(!above_in.is_valid(&args), "{context}: schema accepted it");
            assert!(
                serde_json::from_value::<posts_above::Args>(args).is_err(),
                "{context}: serde accepted it"
            );
        }
    }
}

#[test]
fn required_and_optional_follow_serde() {
    let (fetch, import) = (validator("fetchPost", true), validator("importPost", false));
    let post = post_with(1, None);
    // `quota` is optional: omitted and `null` are both fine.
    for written in [
        edited(&post, "quota", None),
        edited(&post, "quota", Some(Value::Null)),
    ] {
        assert_valid(&fetch, &written, "Post without a quota");
        assert!(serde_json::from_value::<Post>(written).is_ok());
    }
    // `reads` is required, and `null` is not a `BigInt`.
    for written in [
        edited(&post, "reads", None),
        edited(&post, "reads", Some(Value::Null)),
    ] {
        assert!(!fetch.is_valid(&written));
        assert!(!import.is_valid(&json!({ "post": written.clone() })));
        assert!(serde_json::from_value::<Post>(written).is_err());
    }
}

/// The one place the schema is looser than serde: a 19-digit value past
/// `i64` (the pattern cannot enumerate the bound). Pinned exactly.
#[test]
fn out_of_range_values_are_the_one_documented_gap() {
    let import = validator("importPost", false);
    let post = post_with(0, None);
    for text in ["9223372036854775808", "-9223372036854775809"] {
        let written = edited(&post, "reads", Some(json!(text)));
        assert!(
            import.is_valid(&json!({ "post": written.clone() })),
            "{text}: the schema now rejects it; drop the gap"
        );
        assert!(
            serde_json::from_value::<Post>(written).is_err(),
            "{text}: serde now accepts it; drop the gap"
        );
    }
}
