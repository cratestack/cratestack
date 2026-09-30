//! `contract lock|check|prune` against a real lock file.

use super::lock_cmd::{Prune, check, lock, prune};
use super::*;
use cratestack_core::{Schema, client_contract_digest, digest_hex};

const V1: &str = "transport rpc\n\nmodel Widget {\n  id Int @id\n  name String\n}\n\n\
    type Ping {\n  note String\n}\n\nprocedure ping(args: Ping): Ping\n";

fn schema(source: &str) -> Schema {
    cratestack_parser::parse_schema(source).expect("schema parses")
}

fn v2_optional_field() -> String {
    V1.replace("  name String\n", "  name String\n  note String?\n")
}

fn v2_removed_field() -> String {
    V1.replace("  name String\n", "")
}

fn lock_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vaam.contracts.lock");
    (dir, path)
}

#[test]
fn lock_creates_the_file_and_is_idempotent() {
    let (_dir, path) = lock_file();
    let first = lock(&schema(V1), &path, "store 1.0", "2026-10-01").unwrap();
    assert!(
        first.ok && first.text.starts_with("locked generation"),
        "{}",
        first.text
    );
    let again = lock(&schema(V1), &path, "store 1.0", "2026-10-02").unwrap();
    assert!(
        again.ok && again.text.starts_with("already locked"),
        "{}",
        again.text
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"locked_at\": \"2026-10-01\"") && text.contains("store 1.0"));
    assert_eq!(text.matches("\"locked_at\"").count(), 1);
}

#[test]
fn check_passes_when_locked_and_fails_when_not() {
    let (_dir, path) = lock_file();
    lock(&schema(V1), &path, "", "2026-10-01").unwrap();
    let ok = check(&schema(V1), &path, false).unwrap();
    assert!(
        ok.ok && ok.text.starts_with("contract lock OK"),
        "{}",
        ok.text
    );

    // A compatible edit: fine to ship the server, but the new client
    // contract is not a locked generation yet.
    let edited = schema(&v2_optional_field());
    let unlocked = check(&edited, &path, false).unwrap();
    assert!(
        !unlocked.ok && unlocked.text.contains("is not locked"),
        "{}",
        unlocked.text
    );
    lock(&edited, &path, "", "2026-10-02").unwrap();
    assert!(check(&edited, &path, false).unwrap().ok);
}

#[test]
fn check_fails_on_an_incompatible_entry_and_names_the_op_and_reason() {
    let (_dir, path) = lock_file();
    lock(&schema(V1), &path, "", "2026-10-01").unwrap();
    let broken = schema(&v2_removed_field());
    let out = check(&broken, &path, false).unwrap();
    assert!(!out.ok);
    assert!(
        out.text.contains("model.Widget.create") && out.text.contains("`Widget.name` was removed")
    );
    let json: serde_json::Value =
        serde_json::from_str(&check(&broken, &path, true).unwrap().text).unwrap();
    assert_eq!(json["ok"], false);
    assert!(
        json["incompatible"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["op"] == "model.Widget.create")
    );
    assert_eq!(json["locked"], false);
}

#[test]
fn lock_refuses_until_the_broken_ops_are_pruned() {
    let (_dir, path) = lock_file();
    lock(&schema(V1), &path, "", "2026-10-01").unwrap();
    let broken = schema(&v2_removed_field());
    let before = std::fs::read_to_string(&path).unwrap();
    let refused = lock(&broken, &path, "", "2026-10-02").unwrap();
    assert!(
        !refused.ok && refused.text.contains("breaks locked"),
        "{}",
        refused.text
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "untouched");

    let ops = ["create", "update", "list", "get", "delete"];
    for op in ops {
        prune(&path, Prune::Op(format!("model.Widget.{op}"))).unwrap();
    }
    assert!(lock(&broken, &path, "", "2026-10-02").unwrap().ok);
    assert!(check(&broken, &path, false).unwrap().ok);
}

#[test]
fn prune_by_generation_date_and_count() {
    let (_dir, path) = lock_file();
    lock(&schema(V1), &path, "a", "2026-10-01").unwrap();
    lock(&schema(&v2_optional_field()), &path, "b", "2026-10-05").unwrap();
    let hex = digest_hex(&client_contract_digest(&schema(V1)));
    assert!(prune(&path, Prune::Generation(hex[..4].to_owned())).is_err());
    assert!(prune(&path, Prune::Op("procedure.nope".to_owned())).is_err());
    let out = prune(&path, Prune::Before("2026-10-03".to_owned())).unwrap();
    assert!(out.text.starts_with("pruned 1 generation"), "{}", out.text);
    let out = prune(&path, Prune::Keep(1)).unwrap();
    assert!(out.text.starts_with("pruned 0 generation"), "{}", out.text);
    let out = prune(
        &path,
        Prune::Generation(
            digest_hex(&client_contract_digest(&schema(&v2_optional_field())))[..12].to_owned(),
        ),
    )
    .unwrap();
    assert!(out.text.starts_with("pruned 1 generation"), "{}", out.text);
}

#[test]
fn a_missing_or_tampered_lock_is_an_error_not_a_pass() {
    let (_dir, path) = lock_file();
    assert!(check(&schema(V1), &path, false).is_err(), "missing");
    std::fs::write(&path, "{\"format\":1}").unwrap();
    assert!(check(&schema(V1), &path, false).is_err(), "not a lock");
}
