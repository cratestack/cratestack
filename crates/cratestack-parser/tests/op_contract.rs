//! Per-op contract digests over parsed source (cratestack#1123): edits that
//! leave an op's wire shape alone must not move its digest, and edits that
//! change a shape move exactly the ops whose closure reaches it.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{digest_hex, op_contract_digests};

const BASE: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

transport rpc

auth Principal {
  id Int
}

enum Status {
  Open
  Closed
}

model Account {
  id     Int    @id
  name   String @length(min: 1, max: 40)
  status Status
  orders Order[] @relation(fields: [id], references: [accountId])

  @@allow("read", auth() != null)
}

model Order {
  id        Int @id
  accountId Int
  total     Int
  account   Account @relation(fields: [accountId], references: [id])
}

model Note {
  id   Int    @id
  body String
}

type PingArgs {
  message String
}

type PingReply {
  echo String
}

procedure ping(args: PingArgs): PingReply
  @allow(auth() != null)
"#;

fn digests(source: &str) -> BTreeMap<String, String> {
    let schema = cratestack_parser::parse_schema(source).expect("schema parses");
    op_contract_digests(&schema)
        .into_iter()
        .map(|(key, digest)| (key, digest_hex(&digest)))
        .collect()
}

fn edit(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "base has no `{from}`");
    BASE.replacen(from, to, 1)
}

/// Keys present in both whose digest differs.
fn moved(source: &str) -> BTreeSet<String> {
    let (before, after) = (digests(BASE), digests(source));
    before
        .iter()
        .filter(|(key, digest)| after.get(*key).is_some_and(|d| d != *digest))
        .map(|(key, _)| key.clone())
        .collect()
}

fn ops_of(models: &[&str]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for model in models {
        for verb in ["list", "get", "create", "update", "delete"] {
            keys.insert(format!("model.{model}.{verb}"));
        }
    }
    keys
}

fn assert_unchanged(source: &str) {
    assert_eq!(digests(source), digests(BASE));
}

#[test]
fn a_policy_edit_moves_nothing() {
    assert_unchanged(&edit(
        r#"@@allow("read", auth() != null)"#,
        "@@allow(\"read\", auth().id == 1)\n  @@deny(\"update\", true)",
    ));
}

#[test]
fn an_index_audit_and_soft_delete_move_nothing() {
    assert_unchanged(&edit(
        r#"@@allow("read", auth() != null)"#,
        "@@allow(\"read\", auth() != null)\n  @@index([name])\n  @@audit\n  @@soft_delete",
    ));
}

#[test]
fn a_validator_edit_moves_nothing() {
    assert_unchanged(&edit("@length(min: 1, max: 40)", "@length(min: 2, max: 400)"));
}

#[test]
fn procedure_policy_and_idempotency_markers_move_nothing() {
    assert_unchanged(&edit(
        "  @allow(auth() != null)\n",
        "  @allow(auth() != null)\n  @deny(auth().id == 0)\n  @no_idempotency\n",
    ));
}

#[test]
fn the_auth_block_and_datasource_move_nothing() {
    let source = edit("  id Int\n}\n\nenum", "  id Int\n  role String?\n}\n\nenum");
    assert_unchanged(&source.replace("DATABASE_URL", "OTHER_URL"));
}

#[test]
fn a_server_only_field_moves_nothing() {
    assert_unchanged(&edit(
        "  total     Int\n",
        "  total     Int\n  ledger    String @server_only\n",
    ));
}

#[test]
fn docs_comments_whitespace_and_order_move_nothing() {
    let reordered = format!(
        "{}\n// trailing comment\n",
        edit(
            "model Note {\n  id   Int    @id\n  body String\n}",
            "/// Notes.\nmodel Note {\n  body   String\n  id Int @id\n}"
        )
    );
    assert_unchanged(&reordered);
}

#[test]
fn a_new_model_type_procedure_and_unreachable_view_move_no_existing_op() {
    let source = format!(
        "{BASE}\nmodel Extra {{\n  id Int @id\n}}\n\ntype Other {{\n  n Int\n}}\n\n\
         procedure pong(args: Other): Other\n  @allow(auth() != null)\n\n\
         view NoteBody from Note {{\n  id Int @id @from(Note.id)\n  @@sql(\"SELECT id FROM notes\")\n}}\n"
    );
    let (before, after) = (digests(BASE), digests(&source));
    for (key, digest) in &before {
        assert_eq!(after.get(key), Some(digest), "{key}");
    }
    assert!(after.len() > before.len());
}

#[test]
fn a_retype_moves_the_model_and_whatever_reaches_it() {
    // Order reaches Account through its relation; Note and ping do not.
    let source = edit("name   String @length(min: 1, max: 40)", "name   Int");
    assert_eq!(moved(&source), ops_of(&["Account", "Order"]));
}

#[test]
fn a_field_add_moves_only_that_models_ops() {
    let source = edit("body String\n}", "body String\n  pinned Boolean @default(false)\n}");
    assert_eq!(moved(&source), ops_of(&["Note"]));
}

#[test]
fn a_field_removal_moves_only_that_models_ops() {
    let source = edit("  body String\n}", "}");
    assert_eq!(moved(&source), ops_of(&["Note"]));
}

#[test]
fn an_enum_variant_add_moves_the_ops_that_reach_the_enum() {
    let source = edit("Closed\n}", "Closed\n  Archived\n}");
    assert_eq!(moved(&source), ops_of(&["Account", "Order"]));
}

#[test]
fn an_enum_variant_reorder_moves_the_ops_that_reach_the_enum() {
    let source = edit("Open\n  Closed", "Closed\n  Open");
    assert_eq!(moved(&source), ops_of(&["Account", "Order"]));
}

#[test]
fn an_arity_change_moves_only_that_models_ops() {
    let source = edit("body String\n}", "body String?\n}");
    assert_eq!(moved(&source), ops_of(&["Note"]));
}

#[test]
fn paged_moves_its_models_ops_only() {
    let source = edit("  body String\n}", "  body String\n\n  @@paged\n}");
    assert_eq!(moved(&source), ops_of(&["Note"]));
}

#[test]
fn an_argument_rename_moves_only_that_procedure() {
    let source = edit("message String", "text String");
    assert_eq!(moved(&source), BTreeSet::from(["procedure.ping".to_owned()]));
}

#[test]
fn a_default_on_a_create_input_field_moves_that_model_only() {
    let source = edit("body String\n}", "body String @default(\"\")\n}");
    assert_eq!(moved(&source), ops_of(&["Note"]));
}

#[test]
fn a_field_attribute_off_the_drop_list_moves_its_ops() {
    let source = edit("body String\n}", "body String @unique\n}");
    assert_eq!(moved(&source), ops_of(&["Note"]));
}

#[test]
fn subscribe_carries_its_event_kinds_and_nothing_else_does() {
    let with = |events: &str| {
        edit(
            "  body String\n}",
            &format!("  body String\n\n  @@emit({events})\n  @@subscribe\n}}"),
        )
    };
    let (one, two) = (digests(&with("created")), digests(&with("created, deleted")));
    let changed: BTreeSet<_> = one
        .iter()
        .filter(|(k, d)| two[*k] != **d)
        .map(|(k, _)| k.clone())
        .collect();
    assert_eq!(changed, BTreeSet::from(["model.Note.subscribe".to_owned()]));
    // The emit list is not part of the model's own projection.
    assert_eq!(one["model.Note.list"], digests(BASE)["model.Note.list"]);
}
