//! Runtime proof for issue #304's AC #5 ("The plain functions are
//! genuinely framework-free — no React import, no hook, usable from
//! Node. This is proven by a test that imports and calls one outside
//! any React context.") — `tests/swr_generator.rs` already proves the
//! static half (no `"react"`/hook text in generated `.ts` sources); this
//! proves it by actually running one.
//!
//! This generates the `swr` preset's `tiny_rest` package to a temp
//! directory, starts a real local HTTP stub server, and runs a small
//! script (via `tsx` — a plain TS runner, not a UI framework — resolved by
//! `tests/support`, see cratestack#738) that
//! imports `getWidget` and calls it against that server — if the
//! generated code accidentally pulled in React or any other framework
//! dependency, module resolution would fail outright, not just silently
//! succeed.
//!
//! cratestack#499 (the `swr` preset's F3 decode-side revival fix): unlike
//! before, `getWidget` (and every generated model function) now really
//! does `import { reviveDecimalFields } from "./shared.js"` — a genuine
//! (not type-only) import — which in turn genuinely imports `decimal.js`,
//! regardless of whether `Widget` itself has a `Decimal` field. `npm
//! install` is required before running the smoke script now, mirroring
//! `tests/rest_list_query_wire_format.rs`'s own identical fix for the
//! `default` preset's `client.ts` (see that test's own comment on the
//! exact same failure mode: `tsx` hangs rather than failing fast when
//! it can't resolve `decimal.js` from a `node_modules` that was never
//! installed — confirmed empirically here too). This test still proves
//! AC #5's actual claim (no React/hook import anywhere in the invoked
//! module graph) — `decimal.js` is a deliberate, disclosed runtime
//! dependency every generated package has needed since cratestack#498,
//! not a framework.
//!
//! No Rust CI job in this repo currently provisions Node (`js` is the
//! only one, and it doesn't run `cargo test`) — see `.github/workflows/ci.yml`.
//! So this test degrades to a skip (printed, not silently swallowed) when
//! `node`/`npm` aren't on `PATH`, rather than failing a CI job that was
//! never going to have them. Where Node *is* available (local dev, or a
//! future CI job that adds it), this is a real, non-trivial verification.

use std::io::Write as _;
use std::net::TcpListener;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

#[test]
fn generated_plain_function_runs_outside_any_react_context() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping generated_plain_function_runs_outside_any_react_context: \
             `node`/`npm` not on PATH (expected in this repo's Rust-only CI jobs — \
             see tests/swr_runtime.rs's module doc)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file("tests/fixtures/tiny_rest.cstack")
        .expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-runtime-check".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--swr should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    // cratestack#499: see this file's own module doc — `getWidget`'s
    // module graph now genuinely imports `decimal.js`, so `npx tsx` needs
    // a real `node_modules` to resolve it against.
    let mut install = Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed:\n{}",
        command_report(&install, &installed)
    );

    // A real stub server, not a mock — the function under test does a
    // genuine `fetch()`.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
    let port = listener.local_addr().expect("local addr").port();
    let server = std::thread::spawn(move || serve_one_widget_request(listener));

    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ CratestackRuntime }} from "./src/swr/runtime";
import {{ getWidget }} from "./src/swr/models/widget";

const runtime = new CratestackRuntime("http://127.0.0.1:{port}", {{ basePath: "/api" }});
const widget = await getWidget(runtime, 1);
if (widget.name !== "Test Widget") {{
  throw new Error(`unexpected widget: ${{JSON.stringify(widget)}}`);
}}
console.log("SWR_RUNTIME_CHECK_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.ts");
    let output = tsx.output().expect("run tsx");

    // THE STATUS CHECK COMES BEFORE THE JOIN, AND THAT ORDER IS THE WHOLE
    // POINT. The stub server thread is parked in `accept()`; if the smoke
    // script died before issuing its request, nothing ever connects and this
    // `join()` never returns. Asserting afterwards leaves the stderr that says
    // WHY it died sitting unread in `output` while the test hangs.
    // Three CI runs sat exactly like that for over three hours each on
    // 2026-08-24 (main `afdcd9ce`, jobs 97452966528 and 97485030107) before
    // being cancelled by hand, and the trigger is STILL unknown because the
    // message was never printed. Assert first, then join.
    assert!(
        output.status.success(),
        "generated plain function failed to run under plain Node (tsx, no React/hooks \
         anywhere in the invoked module graph):\n{}",
        command_report(&tsx, &output)
    );

    server.join().expect("stub server thread");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SWR_RUNTIME_CHECK_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );
}

/// Accepts exactly one HTTP connection, replies to any request with the
/// fixture's `Widget` JSON shape, then returns. Hand-rolled instead of
/// pulling in an HTTP server crate — this only needs to prove one real
/// `fetch()` round-trip.
fn serve_one_widget_request(listener: TcpListener) {
    use std::io::{BufRead, BufReader, Write};

    let stream = accept_within(&listener, STUB_ACCEPT_TIMEOUT);
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    // Drain the request line + headers; the response is the same
    // regardless of path/method for this single-shot stub.
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read request line");
        if read == 0 || line == "\r\n" {
            break;
        }
    }

    let body = r#"{"id":1,"name":"Test Widget","weight":null}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = stream;
    stream
        .write_all(response.as_bytes())
        .expect("write stub response");
    stream.flush().expect("flush stub response");
}

/// Generous next to a healthy run (these round trips take ~5s end to end) and
/// tiny next to the six hours an unbounded `accept()` costs.
const STUB_ACCEPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// `accept()` with a deadline, because a blocking one turns "the client never
/// connected" into a hang with no output at all.
///
/// The status assert above catches the common case — the smoke script failed
/// and said why. This covers the rest: a script that exits 0 without issuing a
/// request, or a runtime that never starts. Neither should cost a CI runner six
/// hours, which is the default job timeout a hang runs into.
fn accept_within(
    listener: &std::net::TcpListener,
    timeout: std::time::Duration,
) -> std::net::TcpStream {
    let deadline = std::time::Instant::now() + timeout;
    listener
        .set_nonblocking(true)
        .expect("set listener nonblocking");
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // An accepted stream can inherit the listener's nonblocking
                // flag, and every reader below this is blocking — so clear it
                // explicitly rather than relying on platform behaviour.
                stream
                    .set_nonblocking(false)
                    .expect("clear stream nonblocking");
                listener
                    .set_nonblocking(false)
                    .expect("restore listener blocking");
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "no client connected to the stub server within {timeout:?} — the smoke \
                     script almost certainly failed or exited before issuing its request"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("accept stub connection: {e}"),
        }
    }
}
