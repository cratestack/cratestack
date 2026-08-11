//! `AuditEntry::unsafe_write` coverage (cratestack#507 finding 3),
//! split out of `tests.rs` to stay under the crate's ~200-LoC file
//! convention.

use super::*;

/// The bypass flag must survive a JSONL write + replay round-trip, not
/// just an in-memory snapshot.
#[test]
fn unsafe_write_flag_survives_a_jsonl_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.jsonl");

    let log = AuditLog::persistent(&path).expect("open");
    log.push(
        "prod",
        "Message",
        AuditOp::Update,
        Some("m1".to_owned()),
        false,
    );
    log.push(
        "prod",
        "Message",
        AuditOp::Delete,
        Some("m1".to_owned()),
        true,
    );
    drop(log);

    let reopened = AuditLog::persistent(&path).expect("reopen");
    let snap = reopened.snapshot(10);
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].op, AuditOp::Delete);
    assert!(snap[0].unsafe_write, "delete entry was pushed as a bypass");
    assert_eq!(snap[1].op, AuditOp::Update);
    assert!(
        !snap[1].unsafe_write,
        "update entry was pushed as an ordinary write"
    );
}

/// A JSONL sidecar written before `unsafe_write` existed must still
/// replay cleanly, defaulting the flag to `false` rather than bricking
/// boot or misclassifying pre-upgrade history as a bypass it could
/// never have chosen (`allow_unsafe_writes` didn't exist for those
/// rows either).
#[test]
fn pre_existing_jsonl_lines_without_unsafe_write_default_to_false() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.jsonl");

    // Hand-write a line in the pre-#507 shape: no `unsafe_write` key.
    std::fs::write(
        &path,
        "{\"id\":1,\"at\":\"2024-01-01T00:00:00Z\",\"target\":\"prod\",\"model\":\"Post\",\
         \"op\":\"CREATE\",\"pk\":\"p1\"}\n",
    )
    .expect("write legacy line");

    let log = AuditLog::persistent(&path).expect("legacy file replays without error");
    let snap = log.snapshot(10);
    assert_eq!(snap.len(), 1);
    assert!(
        !snap[0].unsafe_write,
        "a pre-#507 entry with no unsafe_write key must default to false, not fail to parse"
    );
}

/// `/api/audit`'s wire shape (what `AuditResponse` actually serializes)
/// carries the flag, so a client can distinguish a bypass write without
/// reading the JSONL sidecar directly.
#[test]
fn snapshot_entry_serializes_the_unsafe_write_key() {
    let log = AuditLog::new();
    log.push(
        "prod",
        "Message",
        AuditOp::Update,
        Some("m1".to_owned()),
        true,
    );
    let entry = log.snapshot(1).remove(0);
    let value = serde_json::to_value(&entry).expect("entry serializes");
    assert_eq!(value["unsafe_write"], true);
}
