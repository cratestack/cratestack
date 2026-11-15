//! `--require-version` / `CRATESTACK_REQUIRE_VERSION` (cratestack#1372)
//! gate every subcommand before it runs.

use std::process::{Command, Output};

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_cratestack"));
    c.env_remove("CRATESTACK_REQUIRE_VERSION");
    c
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn unsatisfied_requirement_fails_before_the_subcommand_runs() {
    // The schema path does not exist: reaching `check` would fail
    // differently, so the message proves the gate ran first.
    let out = bin()
        .args([
            "--require-version",
            "0.0.1",
            "check",
            "--schema",
            "nope.cstack",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains(env!("CARGO_PKG_VERSION")), "{err}");
    assert!(err.contains("0.0.1") && err.contains("PATH"), "{err}");
}

#[test]
fn env_var_gates_and_a_matching_flag_overrides_it() {
    let out = bin()
        .env("CRATESTACK_REQUIRE_VERSION", "0.0.1")
        .args(["check", "--schema", "nope.cstack"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("does not satisfy"));

    let out = bin()
        .env("CRATESTACK_REQUIRE_VERSION", "0.0.1")
        .args([
            "--require-version",
            env!("CARGO_PKG_VERSION"),
            "check",
            "--schema",
            "nope.cstack",
        ])
        .output()
        .unwrap();
    assert!(
        !stderr(&out).contains("does not satisfy"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn unparsable_requirement_exits_non_zero() {
    let out = bin()
        .args(["--require-version", "garbage!", "check", "--schema", "x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("invalid version requirement"));
}
