//! `@default(...)` marker calls: `dbgenerated()` and the refused
//! `autoincrement()` (cratestack#1128).

use crate::parse_schema;

/// cratestack#1128: a space between the marker name and its `(` must not
/// defeat the refusal — `attribute_text::split_attributes` preserves the
/// space, so `autoincrement ()` is the same broken spelling as
/// `autoincrement()`.
#[test]
fn rejects_autoincrement_default_with_space_before_parens() {
    let error = parse_schema(
        r#"
model Widget {
  id Int @id @default(autoincrement ())
}
"#,
    )
    .expect_err("@default(autoincrement ()) should be refused");

    let message = error.to_string();
    assert!(
        message.contains("autoincrement ()"),
        "error echoes the author's own spelling: {message}",
    );
    assert!(
        message.contains("dbgenerated()"),
        "error points at the marker to use instead: {message}",
    );
}

/// The sibling hole, cratestack#1128: a *bare* `dbgenerated ()` is the same
/// marker as `dbgenerated()` and must stay accepted; only an argument is
/// refused. A space between name and paren must not break either reading.
#[test]
fn spaced_bare_dbgenerated_is_still_the_marker() {
    parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated ())
}
"#,
    )
    .expect("bare @default(dbgenerated ()) is the marker and should parse");
}

/// ...but an *argument* with the same spacing must still be refused (the
/// marker's whole point is that it takes none).
#[test]
fn rejects_dbgenerated_with_argument_and_space() {
    let error = parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated ("gen_random_uuid()"))
}
"#,
    )
    .expect_err("dbgenerated (<args>) with a space should fail");

    assert!(
        error.to_string().contains("takes no argument"),
        "error: {error}",
    );
}

/// cratestack#1128: `autoincrement()` is not a real database function, so
/// `migrate diff` used to pass it through into `DEFAULT autoincrement()`
/// — DDL both Postgres and SQLite reject at apply time, after `check` had
/// already said `schema OK`. It must be refused at the field, pointing at
/// the marker that means "the database generates this value".
#[test]
fn rejects_autoincrement_default_pointing_at_dbgenerated() {
    let error = parse_schema(
        r#"
model Widget {
  id Int @id @default(autoincrement())
}
"#,
    )
    .expect_err("@default(autoincrement()) should be refused");

    let message = error.to_string();
    assert!(
        message.contains("autoincrement()"),
        "error names the spelling: {message}",
    );
    assert!(
        message.contains("dbgenerated()"),
        "error points at the marker to use instead: {message}",
    );
    assert!(
        message.contains("Widget.id"),
        "error names the field: {message}",
    );
}

#[test]
fn rejects_autoincrement_default_with_argument() {
    let error = parse_schema(
        r#"
model Widget {
  id Int @id @default(autoincrement(1))
}
"#,
    )
    .expect_err("@default(autoincrement(1)) should be refused");

    assert!(
        error.to_string().contains("autoincrement(1)"),
        "error echoes the argument form: {error}",
    );
}

/// The refusal must not swallow the legal spellings around it.
#[test]
fn autoincrement_refusal_leaves_dbgenerated_and_now_untouched() {
    parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated())
  createdAt DateTime @default(now())
  status String @default('pending')
}
"#,
    )
    .expect("dbgenerated(), now() and literal defaults must stay valid");
}

/// `dbgenerated()` must be the whole expression: anything after the call
/// would be dropped from the migration without a word, so it is refused
/// (`dbgenerated()` itself classifies as the marker either way).
#[test]
fn rejects_text_after_dbgenerated() {
    for default in [
        "dbgenerated() || 'x'",
        "dbgenerated()::text",
        "dbgenerated() + 1",
    ] {
        let source = format!("model Article {{\n  id String @id @default({default})\n}}\n");
        let error = parse_schema(&source).expect_err(default);
        let message = error.to_string();
        assert!(
            message.contains("whole default expression"),
            "{default}: {message}",
        );
        assert!(message.contains(default), "{default} echoed: {message}");
    }
}

/// An unclosed call is not a bare marker either.
#[test]
fn rejects_unclosed_dbgenerated() {
    parse_schema("model Article {\n  id String @id @default(dbgenerated()\n}\n")
        .expect_err("an unclosed dbgenerated( should fail");
}
