//! `subscription procedure` and `topic` (slice S4).

use crate::parse_schema;

const TYPES: &str = r#"
type RoomArgs {
  roomId String
}

type ChatMessage {
  id Int
  roomId String
  body String
}
"#;

fn schema_with(declarations: &str) -> Result<cratestack_core::Schema, String> {
    parse_schema(&format!("{TYPES}\n{declarations}")).map_err(|error| error.to_string())
}

#[test]
fn parses_a_subscription_procedure_and_a_topic_into_their_own_lists() {
    let schema = schema_with(
        r#"
/// A room's messages.
topic RoomMessage(roomId: String): ChatMessage

/// Live messages of one room.
subscription procedure roomMessages(args: RoomArgs): ChatMessage
  @allow(auth() != null)
"#,
    )
    .expect("parses");
    assert!(schema.procedures.is_empty(), "no unary procedure");
    let procedure = &schema.subscription_procedures[0];
    assert_eq!(procedure.name, "roomMessages");
    assert_eq!(procedure.return_type.name, "ChatMessage");
    assert_eq!(procedure.attributes[0].raw, "@allow(auth() != null)");
    assert_eq!(procedure.docs, ["Live messages of one room."]);
    let topic = &schema.topics[0];
    assert_eq!(
        (
            topic.name.as_str(),
            topic.key.name.as_str(),
            topic.key.ty.name.as_str()
        ),
        ("RoomMessage", "roomId", "String")
    );
    assert_eq!(topic.payload.name, "ChatMessage");
}

#[test]
fn a_subscription_returns_one_item_type() {
    let error = schema_with("subscription procedure feed(args: RoomArgs): ChatMessage[]")
        .expect_err("a list item type is refused");
    assert!(error.contains("must return a single item type"), "{error}");
}

#[test]
fn unary_attributes_are_refused_on_a_subscription() {
    for (attribute, why) in [
        ("@stream", "already a stream"),
        ("@isolation(\"serializable\")", "no transaction open"),
        ("@no_idempotency", "no replayable result"),
    ] {
        let error = schema_with(&format!(
            "subscription procedure feed(args: RoomArgs): ChatMessage\n  {attribute}"
        ))
        .expect_err(attribute);
        assert!(error.contains(why), "{attribute}: {error}");
    }
}

#[test]
fn a_subscription_cannot_share_a_procedures_name() {
    let error = schema_with(
        "procedure feed(args: RoomArgs): ChatMessage\nsubscription procedure feed(args: RoomArgs): ChatMessage",
    )
    .expect_err("same op id");
    assert!(error.contains("duplicate procedure name `feed`"), "{error}");
}

#[test]
fn a_topic_key_names_a_channel() {
    let error = schema_with("topic RoomMessage(room: RoomArgs): ChatMessage")
        .expect_err("a type is not a key");
    assert!(
        error.contains("must be one of String, Int, Uuid"),
        "{error}"
    );
    let error =
        schema_with("topic RoomMessage(a: String, b: String): ChatMessage").expect_err("two keys");
    assert!(error.contains("exactly one key"), "{error}");
    let error = schema_with("topic RoomMessage(roomId: String): Missing").expect_err("unknown");
    assert!(error.contains("Missing"), "{error}");
}
