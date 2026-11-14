//! cratestack#1303: `check` and `print-ir` on a schema whose procedures carry
//! the device-auth marker. These run the real binary and assert stdout,
//! stderr, exit codes and the JSON diagnostic.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIXTURE: &str = include_str!("../../cratestack-parser/tests/fixtures/device_auth.cstack");

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cratestack"))
        .env("RUST_BACKTRACE", "0")
        .args(args)
        .output()
        .expect("the cratestack binary runs")
}

/// A temp dir with `schema.cstack` holding `source`.
fn schema(source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema.cstack");
    std::fs::write(&path, source).unwrap();
    (dir, path)
}

fn arg(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn check_accepts_the_marked_schema() {
    let (_dir, path) = schema(FIXTURE);
    let out = run(&["check", "--schema", &arg(&path)]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("schema OK"));
}

#[test]
fn check_reports_a_public_revoke_at_the_marker_with_its_line() {
    let source = FIXTURE.replace(
        "@device_auth(role: \"revoke\")\n  @allow(auth() != null)",
        "@device_auth(role: \"revoke\")\n  @allow(true)",
    );
    assert_ne!(source, FIXTURE);
    let (_dir, path) = schema(&source);
    let marker_line = source
        .lines()
        .position(|line| line.contains("role: \"revoke\""))
        .unwrap()
        + 1;

    let human = run(&["check", "--schema", &arg(&path)]);
    assert!(!human.status.success());
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert!(stderr.contains("`revoke` must not be public"), "{stderr}");

    let json = run(&["check", "--schema", &arg(&path), "--format", "json"]);
    assert!(!json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).expect("stdout is JSON");
    assert_eq!(value["ok"], false);
    let diagnostic = &value["diagnostics"][0];
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("`revoke` must not be public"),
        "{diagnostic}"
    );
    assert_eq!(diagnostic["line"], marker_line);
}

#[test]
fn check_names_the_missing_roles_and_a_misspelled_attribute() {
    let source = FIXTURE.replace("  @device_auth(role: \"confirmRenewal\")\n", "");
    let (_dir, path) = schema(&source);
    let out = run(&["check", "--schema", &arg(&path)]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("but not `confirmRenewal`"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let source = FIXTURE.replace(
        "@device_auth(role: \"revoke\")",
        "@deviceAuth(role: \"revoke\")",
    );
    let (_dir, path) = schema(&source);
    let out = run(&["check", "--schema", &arg(&path)]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("did you mean `@device_auth`?"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn print_ir_shows_the_markers_as_the_attributes_they_are() {
    let (_dir, path) = schema(FIXTURE);
    let out = run(&["print-ir", "--schema", &arg(&path), "--no-spans"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    for role in [
        "sendEnrolmentCode",
        "beginEnrolment",
        "confirmEnrolment",
        "beginRenewal",
        "confirmRenewal",
        "revoke",
    ] {
        let marker = format!("@device_auth(role: \\\"{role}\\\")");
        assert!(stdout.contains(&marker), "print-ir shows {marker}");
    }
}
