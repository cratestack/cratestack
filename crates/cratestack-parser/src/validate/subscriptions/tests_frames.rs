//! `@frames(unsigned)` on a `subscription procedure` (cratestack#1378,
//! part A): what it accepts and every refusal, with the exact message.

use cratestack_core::FramesMode;

use crate::parse_schema;

const TYPES: &str = r#"
type RoomArgs {
  roomId String
}

type ChatMessage {
  id Int
  body String
}
"#;

fn parse(declarations: &str) -> Result<cratestack_core::Schema, String> {
    parse_schema(&format!("{TYPES}\n{declarations}")).map_err(|error| error.to_string())
}

fn subscription(attributes: &[&str]) -> String {
    let lines: String = attributes.iter().map(|a| format!("\n  {a}")).collect();
    format!("subscription procedure feed(args: RoomArgs): ChatMessage{lines}")
}

fn refused(declarations: &str) -> String {
    parse(declarations).expect_err("refused")
}

#[test]
fn unsigned_is_accepted_and_recorded() {
    let schema = parse(&subscription(&[
        "@allow(auth() != null)",
        "@frames(unsigned)",
    ]))
    .expect("parses");
    let procedure = &schema.subscription_procedures[0];
    assert_eq!(procedure.frames(), Some(FramesMode::Unsigned));
    assert!(
        procedure
            .attributes
            .iter()
            .any(|a| a.raw == "@frames(unsigned)")
    );
}

#[test]
fn no_frames_attribute_is_no_frames_mode() {
    let schema = parse(&subscription(&["@allow(auth() != null)"])).expect("parses");
    assert_eq!(schema.subscription_procedures[0].frames(), None);
}

#[test]
fn sealed_is_reserved_for_signed_frames() {
    let error = refused(&subscription(&["@frames(sealed)"]));
    assert!(
        error.contains(
            "subscription procedure `feed`: `@frames(sealed)` is reserved for signed frames \
             (cratestack#1020) and is not supported yet; `@frames(unsigned)` is the only mode"
        ),
        "{error}"
    );
}

#[test]
fn chain_is_reserved_for_chained_frames() {
    let error = refused(&subscription(&["@frames(chain)"]));
    assert!(
        error.contains(
            "subscription procedure `feed`: `@frames(chain)` is reserved for chained frames \
             (cratestack#1008) and is not supported yet; `@frames(unsigned)` is the only mode"
        ),
        "{error}"
    );
}

#[test]
fn any_other_argument_is_refused() {
    let error = refused(&subscription(&["@frames(signed)"]));
    assert!(
        error.contains(
            "subscription procedure `feed`: `@frames(signed)` is not a frames mode: \
             write `@frames(unsigned)`"
        ),
        "{error}"
    );
    let error = refused(&subscription(&["@frames(unsigned, sealed)"]));
    assert!(error.contains("is not a frames mode"), "{error}");
}

#[test]
fn a_bare_or_empty_frames_is_refused() {
    let error = refused(&subscription(&["@frames"]));
    assert!(
        error.contains("`@frames` takes an argument list: `@frames(…)`"),
        "{error}"
    );
    let error = refused(&subscription(&["@frames()"]));
    assert!(
        error.contains("`@frames()` has an empty argument list"),
        "{error}"
    );
}

#[test]
fn a_duplicate_frames_is_refused() {
    let error = refused(&subscription(&["@frames(unsigned)", "@frames(unsigned)"]));
    assert!(
        error.contains(
            "subscription procedure `feed` declares more than one `@frames` attribute; \
             write it once"
        ),
        "{error}"
    );
}

#[test]
fn a_unary_procedure_refuses_frames() {
    let error = refused("procedure feed(args: RoomArgs): ChatMessage\n  @frames(unsigned)");
    assert!(
        error.contains("unsupported attribute `@frames` on a procedure"),
        "{error}"
    );
    assert!(error.contains("accepts only"), "{error}");
}

#[test]
fn a_misspelled_frames_on_a_unary_procedure_points_at_it() {
    let error = refused("procedure feed(args: RoomArgs): ChatMessage\n  @frame(unsigned)");
    assert!(
        error.contains("did you mean `@frames`? A procedure does not take it"),
        "{error}"
    );
}

#[test]
fn a_model_and_its_feed_refuse_frames() {
    let error = refused("model Widget {\n  id Int @id\n  @@frames(unsigned)\n}");
    assert!(
        error.contains("unsupported attribute `@@frames`"),
        "{error}"
    );
    let error = parse_schema(
        "transport rpc\n\nmodel Widget {\n  id Int @id\n  @@emit(created)\n  @@subscribe\n  \
         @frames(unsigned)\n}",
    )
    .expect_err("model feeds take no frames mode")
    .to_string();
    assert!(
        error.contains("unsupported model directive `@frames(unsigned)`"),
        "{error}"
    );
}

#[test]
fn a_field_refuses_frames() {
    let error = refused("model Widget {\n  id Int @id\n  name String @frames(unsigned)\n}");
    assert!(error.contains("unsupported attribute `@frames`"), "{error}");
}
