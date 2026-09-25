//! The binary refuses to start rather than fall back to a default key or a
//! default identity. No database: every refusal here happens before the
//! server connects, and `DATABASE_URL` points at a closed port to prove it.
//! If a refusal moved after the connection, the error would name Postgres
//! instead, and the test would fail.

use std::process::{Command, Output};

use mcp_operator_example::token::{STDIO_AUDIENCE, mint};

const BIN: &str = env!("CARGO_BIN_EXE_mcp-operator-example");
const KEY: &str = "test-only signing key, at least 32 bytes long";
const CLOSED_PORT: &str = "postgres://nobody:nobody@127.0.0.1:9/none";
const MINT_TOKEN: [&str; 7] = [
    "mint-token",
    "--audience",
    STDIO_AUDIENCE,
    "--id",
    "u-1",
    "--role",
    "editor",
];

/// Runs the binary with only the variables given, none inherited from the
/// shell running the tests.
fn run(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env_remove("MCP_EXAMPLE_SIGNING_KEY")
        .env_remove("MCP_EXAMPLE_TOKEN")
        .env("DATABASE_URL", CLOSED_PORT);
    for (name, value) in vars {
        command.env(name, value);
    }
    command.output().expect("the example binary runs")
}

fn refused(output: &Output, reason: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "started: {stderr}");
    assert!(
        stderr.contains(reason),
        "expected `{reason}`, got: {stderr}"
    );
    assert!(output.stdout.is_empty(), "wrote to stdout");
}

#[test]
fn stdio_without_a_token_does_not_start() {
    let output = run(&["stdio"], &[("MCP_EXAMPLE_SIGNING_KEY", KEY)]);
    refused(&output, "MCP_EXAMPLE_TOKEN is not set");
}

#[test]
fn stdio_with_a_token_for_another_audience_does_not_start() {
    let token = mint(
        KEY.as_bytes(),
        "http://127.0.0.1:8787/mcp",
        "u-1",
        "editor",
        60,
    );
    let output = run(
        &["stdio"],
        &[
            ("MCP_EXAMPLE_SIGNING_KEY", KEY),
            ("MCP_EXAMPLE_TOKEN", &token),
        ],
    );
    refused(&output, "token audience is not this resource");
}

#[test]
fn stdio_with_a_token_signed_by_another_key_does_not_start() {
    let token = mint(
        b"another key that is also 32 bytes long!",
        STDIO_AUDIENCE,
        "u-1",
        "editor",
        60,
    );
    let output = run(
        &["stdio"],
        &[
            ("MCP_EXAMPLE_SIGNING_KEY", KEY),
            ("MCP_EXAMPLE_TOKEN", &token),
        ],
    );
    refused(&output, "bad signature");
}

#[test]
fn no_mode_starts_without_a_signing_key() {
    let token = mint(KEY.as_bytes(), STDIO_AUDIENCE, "u-1", "editor", 60);
    let stdio = run(&["stdio"], &[("MCP_EXAMPLE_TOKEN", &token)]);
    refused(&stdio, "MCP_EXAMPLE_SIGNING_KEY is not set");
    refused(&run(&["http"], &[]), "MCP_EXAMPLE_SIGNING_KEY is not set");
    refused(&run(&MINT_TOKEN, &[]), "MCP_EXAMPLE_SIGNING_KEY is not set");
}

#[test]
fn a_short_signing_key_is_refused_by_every_mode() {
    let short = [("MCP_EXAMPLE_SIGNING_KEY", "secret")];
    refused(&run(&["http"], &short), "at least 32 bytes");
    let token = mint(KEY.as_bytes(), STDIO_AUDIENCE, "u-1", "editor", 60);
    let stdio = run(
        &["stdio"],
        &[
            ("MCP_EXAMPLE_SIGNING_KEY", "secret"),
            ("MCP_EXAMPLE_TOKEN", &token),
        ],
    );
    refused(&stdio, "at least 32 bytes");
    refused(&run(&MINT_TOKEN, &short), "at least 32 bytes");
}
