#![cfg(test)]
//! `@stream` procedure attribute — see `crate::validate::stream_attribute`
//! and cratestack#282. Split from `tests_procedures.rs` to stay under the
//! per-file line ceiling, matching how that file itself is one directive
//! per concern.

use super::parse_schema;

#[test]
fn parses_stream_attribute_on_list_returning_procedure() {
    let schema = parse_schema(
        r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @stream
"#,
    )
    .expect("procedure with @stream on a list return type should parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        attrs.iter().any(|a| a.raw == "@stream"),
        "procedure attributes should include @stream: {:?}",
        attrs,
    );
}

#[test]
fn rejects_stream_attribute_on_non_list_procedure() {
    let error = parse_schema(
        r#"
type Ping {
  nonce String
}

procedure healthcheck(args: Ping): Ping
  @stream
"#,
    )
    .expect_err("@stream on a non-list-returning procedure should fail");

    assert!(
        error.to_string().contains("does not return a list type"),
        "error: {error}",
    );
}

#[test]
fn rejects_duplicate_stream_attribute() {
    let error = parse_schema(
        r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @stream
  @stream
"#,
    )
    .expect_err("duplicate @stream attributes should fail");

    assert!(
        error.to_string().contains("more than one @stream"),
        "error: {error}",
    );
}

#[test]
fn list_returning_procedure_without_stream_still_parses_unmarked() {
    let schema = parse_schema(
        r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
"#,
    )
    .expect("list-returning procedure without @stream should still parse");

    let attrs = &schema.procedures[0].attributes;
    assert!(
        !attrs.iter().any(|a| a.raw == "@stream"),
        "procedure attributes should not include @stream: {:?}",
        attrs,
    );
}
