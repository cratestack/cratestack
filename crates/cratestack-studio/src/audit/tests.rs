use std::io::Write;

use super::*;

fn push_n(log: &AuditLog, n: usize) {
    for i in 0..n {
        log.push("t", "Post", AuditOp::Create, Some(format!("p{i}")));
    }
}

#[test]
fn push_and_snapshot_return_newest_first() {
    let log = AuditLog::new();
    log.push("t", "Post", AuditOp::Create, Some("p1".to_owned()));
    log.push("t", "Post", AuditOp::Update, Some("p1".to_owned()));
    let snap = log.snapshot(10);
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].op, AuditOp::Update);
    assert_eq!(snap[1].op, AuditOp::Create);
}

#[test]
fn buffer_drops_oldest_past_capacity() {
    let log = AuditLog::new();
    push_n(&log, AuditLog::CAPACITY + 5);
    let snap = log.snapshot(AuditLog::CAPACITY * 2);
    assert_eq!(snap.len(), AuditLog::CAPACITY);
    // The first 5 entries should be gone (oldest dropped).
    assert!(snap.iter().all(|e| e.pk.as_deref() != Some("p0")));
    assert!(snap.iter().all(|e| e.pk.as_deref() != Some("p4")));
    assert!(snap.iter().any(|e| e.pk.as_deref() == Some("p5")));
}

/// The zero-footprint default: an in-memory log must not create the
/// sidecar (or anything else) on disk.
#[test]
fn in_memory_log_writes_nothing_to_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = AuditLog::new();
    push_n(&log, 3);
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read_dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert!(entries.is_empty(), "unexpected files: {entries:?}");
}

#[test]
fn persistent_log_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.jsonl");

    let log = AuditLog::persistent(&path).expect("open");
    log.push("prod", "Post", AuditOp::Create, Some("p1".to_owned()));
    log.push("prod", "Post", AuditOp::Delete, Some("p1".to_owned()));
    drop(log);

    let reopened = AuditLog::persistent(&path).expect("reopen");
    let snap = reopened.snapshot(10);
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].op, AuditOp::Delete);
    assert_eq!(snap[1].op, AuditOp::Create);
    assert_eq!(snap[0].target, "prod");
    assert_eq!(snap[0].pk.as_deref(), Some("p1"));
}

/// A restart must not reuse ids, and the resumed counter has to clear
/// the highest id in the *file* — not merely the highest in the
/// replayed tail, which the capacity cap truncates.
#[test]
fn ids_resume_past_the_whole_file_not_just_the_replayed_tail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.jsonl");

    let written = AuditLog::CAPACITY + 5;
    let log = AuditLog::persistent(&path).expect("open");
    push_n(&log, written);
    drop(log);

    let reopened = AuditLog::persistent(&path).expect("reopen");
    // Only CAPACITY entries are replayed into the ring…
    assert_eq!(
        reopened.snapshot(AuditLog::CAPACITY * 2).len(),
        AuditLog::CAPACITY
    );
    // …but the next id clears every id ever written.
    reopened.push("t", "Post", AuditOp::Update, None);
    let newest = reopened.snapshot(1).remove(0);
    assert_eq!(newest.id, written as u64 + 1);
}

/// A torn final line (Studio killed mid-append) or a hand-edit must not
/// brick boot.
#[test]
fn malformed_lines_are_skipped_and_the_rest_replay() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.jsonl");

    let log = AuditLog::persistent(&path).expect("open");
    log.push("t", "Post", AuditOp::Create, Some("good".to_owned()));
    drop(log);

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("append");
    file.write_all(b"{\"id\":2,\"at\":\"2024-01-0")
        .expect("torn line");
    drop(file);

    let reopened = AuditLog::persistent(&path).expect("reopen despite torn line");
    let snap = reopened.snapshot(10);
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].pk.as_deref(), Some("good"));
}

#[test]
fn opening_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested").join("deeper").join("audit.jsonl");

    let log = AuditLog::persistent(&path).expect("open");
    log.push("t", "Post", AuditOp::Create, None);
    drop(log);

    let body = std::fs::read_to_string(&path).expect("file written");
    assert_eq!(body.lines().count(), 1);
}

/// One JSON object per line, so the file stays `tail -f`-able and
/// `jq`-able rather than becoming a single growing array.
#[test]
fn sink_writes_one_json_object_per_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audit.jsonl");

    let log = AuditLog::persistent(&path).expect("open");
    push_n(&log, 3);
    drop(log);

    let body = std::fs::read_to_string(&path).expect("read");
    assert_eq!(body.lines().count(), 3);
    for line in body.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("line is valid json");
        assert_eq!(value["op"], "CREATE");
    }
}

/// Persistence is opt-in, so failing to honour it must be loud at boot
/// rather than degrading to in-memory behind the operator's back.
#[test]
fn opening_an_unwritable_path_is_a_hard_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A directory can't be opened for append, so this stands in for any
    // "the configured path is not a usable file" case.
    let path = dir.path().join("as-a-directory");
    std::fs::create_dir(&path).expect("mkdir");

    let err = AuditLog::persistent(&path).expect_err("must not silently degrade");
    assert!(
        matches!(
            err,
            AuditStoreError::Open { .. } | AuditStoreError::Read { .. }
        ),
        "unexpected error: {err}"
    );
}
