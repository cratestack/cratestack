//! The stdio contract, observed from outside a real process (ADR 0002 §
//! Transports): stdout carries only MCP messages, `tracing` goes to stderr,
//! and the server exits when stdin closes.
//!
//! `harness = false` (see `Cargo.toml`): libtest prints its own progress to
//! stdout, which would make "only MCP on stdout" unassertable. This binary
//! is its own `main`. Run plainly, it is the test: it re-spawns itself with
//! `CHILD_ENV` set, and the child is the server.

mod support;

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cratestack_core::SystemContext;
use cratestack_mcp::StdioServer;
use serde_json::{Value, json};
use support::FakeTools;
use support::client::meta;

const CHILD_ENV: &str = "CRATESTACK_MCP_STDIO_CHILD";

fn main() {
    if std::env::var_os(CHILD_ENV).is_some() {
        child();
    } else {
        parent();
        println!("stdio_process: ok");
    }
}

/// The server, configured the way the crate docs tell an application to:
/// `tracing` to stderr, at a level that makes it emit something per call.
fn child() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::DEBUG)
        .init();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let server = StdioServer::new(
        FakeTools::default(),
        SystemContext::for_service("stdio-test").into_context(),
    )
    .expect("valid table");
    let outcome = runtime.block_on(server.serve());
    std::process::exit(if outcome.is_ok() { 0 } else { 1 });
}

fn parent() {
    let mut child = Command::new(std::env::current_exe().expect("own path"))
        .env(CHILD_ENV, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the server");

    let requests = [
        ("server/discover", json!({})),
        ("tools/list", json!({})),
        (
            "tools/call",
            json!({ "name": "echo", "arguments": { "text": "hi" } }),
        ),
    ];
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for (id, (method, mut params)) in requests.into_iter().enumerate() {
            params["_meta"] = meta("2026-07-28");
            let line =
                json!({ "jsonrpc": "2.0", "id": id + 1, "method": method, "params": params });
            writeln!(stdin, "{line}").expect("write a request");
        }
        // Dropping stdin closes it: the server must now exit by itself.
    }

    let status = wait(&mut child, Duration::from_secs(20));
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        status.success(),
        "the server exited with {status}; stderr:\n{stderr}"
    );

    let messages: Vec<Value> = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|_| panic!("stdout carried a non-JSON line: {line:?}"))
        })
        .collect();
    assert_eq!(
        messages.len(),
        3,
        "one response per request, nothing else:\n{stdout}"
    );
    for (index, message) in messages.iter().enumerate() {
        assert_eq!(
            message["jsonrpc"], "2.0",
            "not a JSON-RPC message: {message}"
        );
        assert_eq!(message["id"], json!(index + 1));
        assert!(message.get("result").is_some(), "{message}");
    }
    assert_eq!(
        messages[2]["result"]["structuredContent"],
        json!({ "text": "hi" })
    );
    assert!(
        stderr.contains("cratestack mcp tool call completed"),
        "tracing output belongs on stderr, and there was some:\n{stderr}"
    );
}

fn wait(child: &mut std::process::Child, limit: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll the child") {
            return status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            panic!("the server did not exit within {limit:?} of stdin closing");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
