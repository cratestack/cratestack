//! `cratestack contract` exit codes and `check --json` (review of
//! cratestack#1132, S5): 0 ok, 1 a failed verdict (not locked, broken), 2 a
//! tool error (unreadable schema or lock, bad date, bad flag value). With
//! `--json`, `check` prints one JSON document on every path.

use std::path::Path;
use std::process::{Command, Output};

const SCHEMA: &str =
    "transport rpc\n\ntype Ping {\n  note String\n}\n\nprocedure ping(args: Ping): Ping\n";

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cratestack"))
        .arg("contract")
        .args(args)
        .output()
        .expect("the cratestack binary runs")
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("exited, not killed")
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

fn schema_in(dir: &Path) -> String {
    let schema = path(dir, "s.cstack");
    std::fs::write(&schema, SCHEMA).unwrap();
    schema
}

#[test]
fn a_locked_schema_passes_with_exit_0() {
    let dir = tempfile::tempdir().unwrap();
    let (schema, lock) = (schema_in(dir.path()), path(dir.path(), "l.lock"));
    assert_eq!(
        code(&run(&[
            "lock",
            "--schema",
            &schema,
            "--lock",
            &lock,
            "--date",
            "2026-10-02"
        ])),
        0
    );
    let out = run(&["check", "--schema", &schema, "--lock", &lock, "--json"]);
    assert_eq!(code(&out), 0);
    assert_eq!(json(&out)["ok"], true);
}

#[test]
fn an_unlocked_schema_is_a_failed_verdict_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let (schema, lock) = (schema_in(dir.path()), path(dir.path(), "l.lock"));
    std::fs::write(&lock, "{\"format\":1,\"domain\":\"cratestack/op-contract/v1\",\"contracts\":{},\"generations\":[]}").unwrap();
    let out = run(&["check", "--schema", &schema, "--lock", &lock, "--json"]);
    assert_eq!(code(&out), 1);
    assert_eq!(json(&out)["locked"], false);
}

#[test]
fn check_json_is_json_and_exit_2_when_the_lock_is_missing_or_tampered() {
    let dir = tempfile::tempdir().unwrap();
    let schema = schema_in(dir.path());
    let missing = path(dir.path(), "nope.lock");
    let out = run(&["check", "--schema", &schema, "--lock", &missing, "--json"]);
    assert_eq!(code(&out), 2);
    let doc = json(&out);
    assert_eq!(doc["ok"], false);
    assert_eq!(doc["locked"], false);
    assert_eq!(doc["incompatible"], serde_json::json!([]));
    assert!(
        doc["error"].as_str().unwrap().contains("cannot read"),
        "{doc}"
    );
    assert!(doc["client_contract"].is_string());

    let tampered = path(dir.path(), "bad.lock");
    std::fs::write(&tampered, "{ not json").unwrap();
    let out = run(&["check", "--schema", &schema, "--lock", &tampered, "--json"]);
    assert_eq!(code(&out), 2);
    assert_eq!(json(&out)["ok"], false);
}

#[test]
fn check_json_is_json_and_exit_2_when_the_schema_does_not_parse() {
    let dir = tempfile::tempdir().unwrap();
    let schema = path(dir.path(), "broken.cstack");
    std::fs::write(&schema, "model User {\n  email String\n}\n").unwrap();
    let out = run(&[
        "check",
        "--schema",
        &schema,
        "--lock",
        &path(dir.path(), "x.lock"),
        "--json",
    ]);
    assert_eq!(code(&out), 2);
    let doc = json(&out);
    assert_eq!(doc["ok"], false);
    assert!(doc["client_contract"].is_null());
    assert!(doc["error"].is_string());
}

#[test]
fn tool_errors_exit_2_without_json_too() {
    let dir = tempfile::tempdir().unwrap();
    let (schema, lock) = (schema_in(dir.path()), path(dir.path(), "l.lock"));
    let out = run(&["check", "--schema", &schema, "--lock", &lock]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
    assert!(out.stdout.is_empty());
    let out = run(&[
        "lock",
        "--schema",
        &schema,
        "--lock",
        &lock,
        "--date",
        "10/02/2026",
    ]);
    assert_eq!(code(&out), 2);
    assert!(!Path::new(&lock).exists(), "a bad date writes nothing");
}

#[test]
fn prune_before_rejects_a_bad_date_with_exit_2_and_leaves_the_lock_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (schema, lock) = (schema_in(dir.path()), path(dir.path(), "l.lock"));
    assert_eq!(
        code(&run(&[
            "lock",
            "--schema",
            &schema,
            "--lock",
            &lock,
            "--date",
            "2026-10-02"
        ])),
        0
    );
    let before = std::fs::read_to_string(&lock).unwrap();
    let out = run(&["prune", "--lock", &lock, "--before", "10/02/2026"]);
    assert_eq!(code(&out), 2);
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), before);
}
