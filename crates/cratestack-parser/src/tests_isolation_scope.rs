#![cfg(test)]
//! Where `@isolation` is refused (docs/design/procedure-isolation.md §8,
//! GHSA-r67q-4qqq-g9gm). `validate::isolation_scope` holds the rules.

use super::parse_schema;

#[test]
fn rejects_isolation_on_a_stream_procedure() {
    let error = parse_schema(
        r#"
type Tick {
  n Int
}

procedure ticks(args: Tick): Tick[]
  @stream
  @isolation("serializable")
"#,
    )
    .expect_err("@isolation + @stream must be refused");
    assert!(
        error
            .to_string()
            .contains("declares both @isolation and @stream"),
        "error: {error}",
    );
}

#[test]
fn rejects_isolation_without_a_database() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "none"
}

type Ping {
  nonce String
}

procedure ping(args: Ping): Ping
  @isolation("serializable")
"#,
    )
    .expect_err("@isolation under provider = \"none\" must be refused");
    assert!(
        error
            .to_string()
            .contains("there is no database transaction to isolate"),
        "error: {error}",
    );
}

#[test]
fn accepts_isolation_on_a_postgres_schema_and_the_same_stream_without_it() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

type Tick {
  n Int
}

procedure ticks(args: Tick): Tick[]
  @stream

mutation procedure settle(args: Tick): Tick
  @isolation("serializable")
"#,
    )
    .expect("@isolation on a non-stream procedure with a database is fine");
}
