//! Smoke test: generated typed surface from `include_client_schema!` exists
//! and exposes the expected models. Network round-trips are covered by the
//! framework's own integration tests in `crates/cratestack/tests/generated_client_rust.rs`.

use cratestack::include_client_schema;

include_client_schema!("schema.cstack");

#[test]
fn schema_constants_expose_post_model() {
    assert!(cratestack_schema::MODELS.contains(&"Post"));
    assert!(cratestack_schema::TYPES.is_empty());
    assert!(cratestack_schema::PROCEDURES.is_empty());
}

#[test]
fn generated_post_struct_is_serde_round_trippable() {
    let post = cratestack_schema::Post {
        id: 7,
        title: "Hello".into(),
        published: true,
        authorId: 42,
    };
    let json = serde_json::to_string(&post).expect("serialize");
    let parsed: cratestack_schema::Post = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.id, 7);
    assert_eq!(parsed.title, "Hello");
}
