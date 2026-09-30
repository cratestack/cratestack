//! One parsed-source invariance test per entry of the drop list
//! (`DROPPED_ATTRIBUTES`): adding or editing the attribute leaves every
//! existing op's digest where it was. The entries `op_contract.rs` already
//! exercises (`@@allow`, `@@deny`, `@allow`, `@deny`, `@@index`, `@@audit`,
//! `@@soft_delete`, `@length`, `@no_idempotency`, `@@emit`) are not repeated.

mod common;
use common::*;

use cratestack_core::{digest_hex, op_contract_digests};

fn digest_of(source: &str, key: &str) -> String {
    let schema = cratestack_parser::parse_schema(source).expect("schema parses");
    let table = op_contract_digests(&schema);
    digest_hex(
        &table
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("no op {key}"))
            .1,
    )
}

#[test]
fn unique_and_retain_move_nothing() {
    assert_unchanged(&edit(
        "  @@allow(\"read\", auth() != null)\n}",
        "  @@allow(\"read\", auth() != null)\n  @@unique([name, status])\n  @@audit\n  @@retain(days: 30)\n}",
    ));
}

#[test]
fn every_remaining_validator_moves_nothing() {
    for (field, edited) in [
        ("total     Int\n", "total     Int @range(min: 0, max: 9)\n"),
        ("body String\n}", "body String @regex(\"^a\")\n}"),
        ("body String\n}", "body String @email\n}"),
        ("body String\n}", "body String @uri\n}"),
        ("body String\n}", "body String @iso4217\n}"),
    ] {
        assert_unchanged(&edit(field, edited));
    }
}

#[test]
fn internal_removes_ops_but_moves_no_survivor() {
    let (before, after) = (
        digests(BASE),
        digests(&edit(
            "  body String\n}",
            "  body String\n\n  @@internal(\"delete\")\n}",
        )),
    );
    assert!(!after.contains_key("model.Note.delete"));
    for (key, digest) in &after {
        assert_eq!(before.get(key), Some(digest), "{key}");
    }
}

#[test]
fn subscribe_adds_an_op_but_moves_no_existing_one() {
    let after = digests(&edit(
        "  body String\n}",
        "  body String\n\n  @@emit(created)\n  @@subscribe\n}",
    ));
    let before = digests(BASE);
    assert!(after.contains_key("model.Note.subscribe"));
    for (key, digest) in &before {
        assert_eq!(after.get(key), Some(digest), "{key}");
    }
}

const WITH_PROCEDURES: &str = "
type NoteRef {
  id Int
}

mutation procedure touch(args: NoteRef): PingReply
  @allow(auth() != null)
";

fn with_touch(attrs: &str) -> String {
    format!("{BASE}{WITH_PROCEDURES}{attrs}")
}

#[test]
fn authorize_isolation_and_rate_limit_markers_move_nothing() {
    let plain = digest_of(&with_touch(""), "procedure.touch");
    for attrs in [
        "  @authorize(Note, update, args.id)\n",
        "  @isolation(\"serializable\")\n",
    ] {
        assert_eq!(
            digest_of(&with_touch(attrs), "procedure.touch"),
            plain,
            "{attrs}"
        );
    }
    let limited = with_touch("  @no_rate_limit\n").replace(
        "transport rpc\n",
        "transport rpc\n\nextension rate_limit {\n}\n",
    );
    assert_eq!(digest_of(&limited, "procedure.touch"), plain);
}

const VIEWED: &str = "
view NoteBody from Note {
  id   Int    @id @from(Note.id)
  body String @from(Note.body)
  @@SQL@@
}
";

/// The parser refuses a view as an argument, return or field type, so no op
/// can reach one: every view attribute is invisible to every op contract.
#[test]
fn a_views_sql_attributes_move_nothing() {
    for attrs in [
        "@@sql(\"SELECT id, body FROM notes\")",
        "@@sql(\"SELECT id, body FROM other\")",
        "@@server_sql(\"SELECT id, body FROM notes\")\n  @@embedded_sql(\"SELECT id, body FROM notes\")",
        "@@server_sql(\"SELECT id, body FROM notes\")\n  @@materialized",
    ] {
        assert_unchanged(&format!("{BASE}{}", VIEWED.replace("@@SQL@@", attrs)));
    }
}

#[test]
fn a_view_cannot_be_an_ops_type() {
    let source = format!(
        "{BASE}{}\nprocedure body(args: PingArgs): NoteBody\n  @allow(auth() != null)\n",
        VIEWED.replace("@@SQL@@", "@@sql(\"SELECT 1\")")
    );
    assert!(cratestack_parser::parse_schema(&source).is_err());
}

#[test]
fn rename_markers_move_nothing() {
    assert_unchanged(&edit(
        "  body String\n}",
        "  body String @rename(from = \"text\")\n}",
    ));
    assert_unchanged(&edit(
        "  body String\n}",
        "  body String\n\n  @@rename(from = \"Memo\")\n}",
    ));
}

#[test]
fn pii_sensitive_and_unique_fields_move_nothing() {
    for attrs in ["@pii", "@sensitive", "@unique", "@pii @sensitive @unique"] {
        assert_unchanged(&edit(
            "  body String\n}",
            &format!("  body String {attrs}\n}}"),
        ));
    }
}

#[test]
fn db_enforce_moves_nothing() {
    assert_unchanged(&edit(
        "total     Int\n",
        "total     Int @range(min: 0, max: 9) @db_enforce\n",
    ));
}

#[test]
fn deprecated_procedures_keep_their_digest() {
    let plain = digest_of(BASE, "procedure.ping");
    for attr in ["@deprecated", "@deprecated(\"use pong\")"] {
        let source = BASE.replace(
            "procedure ping(args: PingArgs): PingReply\n  @allow(auth() != null)",
            &format!(
                "procedure ping(args: PingArgs): PingReply\n  @allow(auth() != null)\n  {attr}"
            ),
        );
        assert_ne!(source, BASE);
        assert_eq!(digest_of(&source, "procedure.ping"), plain, "{attr}");
    }
}

#[test]
fn a_view_fields_source_moves_nothing() {
    let viewed = |from: &str| {
        format!(
            "{BASE}{}",
            VIEWED
                .replace("@@SQL@@", "@@sql(\"SELECT id, body FROM notes\")")
                .replace("@from(Note.body)", from)
        )
    };
    assert_unchanged(&viewed("@from(Note.body)"));
    assert_unchanged(&viewed("@from(Note.id)"));
}
