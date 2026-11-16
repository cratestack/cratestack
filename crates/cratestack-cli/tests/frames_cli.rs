//! cratestack#1378, part A: `check` accepts `@frames(unsigned)` and refuses
//! the reserved modes, and `print-ir` and `contract print` show the
//! attribute, which is how a reviewer sees why a digest moved.

use std::process::{Command, Output};

const SCHEMA: &str = r#"
transport rpc

datasource db {
  provider = "none"
}

type CountArgs {
  to Int
}

type Tick {
  n Int
}

subscription procedure hinted(args: CountArgs): Tick
  @allow(auth() != null)
  @frames(MODE)
"#;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cratestack"))
        .env("RUST_BACKTRACE", "0")
        .args(args)
        .output()
        .expect("the cratestack binary runs")
}

fn with_mode(mode: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema.cstack");
    std::fs::write(&path, SCHEMA.replace("MODE", mode)).unwrap();
    let path = path.display().to_string();
    (dir, path)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn check_accepts_unsigned_and_refuses_the_reserved_modes() {
    let (_dir, path) = with_mode("unsigned");
    let ok = run(&["check", "--schema", &path]);
    assert!(ok.status.success(), "{}", text(&ok.stderr));
    for (mode, issue) in [("sealed", "#1020"), ("chain", "#1008")] {
        let (_dir, path) = with_mode(mode);
        let refused = run(&["check", "--schema", &path]);
        assert!(!refused.status.success(), "{mode}");
        let stderr = text(&refused.stderr);
        assert!(stderr.contains("is reserved for"), "{stderr}");
        assert!(stderr.contains(issue), "{stderr}");
    }
}

#[test]
fn print_ir_and_contract_print_show_the_attribute() {
    let (_dir, path) = with_mode("unsigned");
    let ir = run(&["print-ir", "--schema", &path]);
    assert!(text(&ir.stdout).contains("@frames(unsigned)"));
    let contract = run(&[
        "contract",
        "print",
        "--schema",
        &path,
        "--op",
        "procedure.hinted",
    ]);
    assert!(contract.status.success(), "{}", text(&contract.stderr));
    assert!(text(&contract.stdout).contains("\"@frames(unsigned)\""));
}
