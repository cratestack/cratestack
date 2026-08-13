//! Real-Node proof for issue #315: a generated package's *compiled*
//! `dist/` output must be importable by plain Node ESM through normal
//! `node_modules` package resolution (`import "<pkg-name>"`), not just by
//! a bundler (Vite/webpack/esbuild) or a TS-aware runner (`tsx`) that
//! tolerates extensionless relative specifiers.
//!
//! `tests/swr_runtime.rs` proves the generated *source* is React-free by
//! running `npx tsx` against a relative path straight into `src/` — that
//! uses bundler-style resolution and never touches `tsc`'s compiled
//! output, so it can't catch a missing `.js` extension on a relative
//! import surviving into `dist/`. This test is the one described in
//! #315's own "Suggested fix": build the package for real (`npm run
//! build`), install it into a separate consumer package via a `file:`
//! dependency so resolution goes through `node_modules`/`exports` like a
//! real consumer, then run plain `node` (not `tsx`, not a bundler)
//! against it.
//!
//! Follows `tests/swr_runtime.rs`'s Node-availability skip convention: no
//! Rust CI job in this repo currently provisions Node, so this degrades
//! to a printed skip rather than failing a job that was never going to
//! have `node`/`npm`/`npx` on `PATH`.

use std::io::Write as _;
use std::net::TcpListener;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn generated_package_compiled_dist_imports_under_plain_node_esm() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping generated_package_compiled_dist_imports_under_plain_node_esm: \
             `node`/`npm`/`npx` not on PATH (expected in this repo's Rust-only CI jobs — \
             see this test's module doc)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file("tests/fixtures/tiny_rest.cstack")
        .expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "node-dist-esm-check".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--swr should render");

    let pkg_dir = tempfile::tempdir().expect("pkg tempdir");
    for file in &package.files {
        let path = pkg_dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    // Peer/dev deps only in the generated manifest (never hard deps) —
    // install them directly, mirroring tests/swr_paged_model_tsc.rs.
    let install_peers = Command::new("npm")
        .args([
            "install",
            "--no-save",
            "--no-audit",
            "--no-fund",
            "typescript@5",
            "swr",
        ])
        .current_dir(pkg_dir.path())
        .output()
        .expect("run npm install (peers)");
    assert!(
        install_peers.status.success(),
        "npm install (peers) failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install_peers.stdout),
        String::from_utf8_lossy(&install_peers.stderr)
    );

    let build = Command::new("npm")
        .args(["run", "build"])
        .current_dir(pkg_dir.path())
        .output()
        .expect("run npm run build");
    assert!(
        build.status.success(),
        "npm run build (tsc) failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    // A separate consumer package, depending on the generated package by
    // `file:` path — this routes resolution through real node_modules /
    // package.json `exports`, not a relative source path, which is the
    // exact gap `tests/swr_runtime.rs` can't cover (see module doc).
    let consumer_dir = tempfile::tempdir().expect("consumer tempdir");
    std::fs::write(
        consumer_dir.path().join("package.json"),
        format!(
            r#"{{
  "name": "node-dist-esm-consumer",
  "version": "0.0.0",
  "private": true,
  "type": "module",
  "dependencies": {{
    "node-dist-esm-check": "file:{}"
  }}
}}
"#,
            pkg_dir.path().display()
        ),
    )
    .expect("write consumer package.json");

    let install_consumer = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund", "--legacy-peer-deps"])
        .current_dir(consumer_dir.path())
        .output()
        .expect("run npm install (consumer)");
    assert!(
        install_consumer.status.success(),
        "npm install (consumer) failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install_consumer.stdout),
        String::from_utf8_lossy(&install_consumer.stderr)
    );

    // A real stub server, not a mock — the function under test does a
    // genuine `fetch()`. If the generated package fails to import at all
    // (the exact regression this test guards against), `node` exits
    // before ever connecting, and `listener.accept()` would block
    // forever — so completion is signalled over a channel and waited on
    // with a bound, not joined unconditionally.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
    let port = listener.local_addr().expect("local addr").port();
    let (server_done_tx, server_done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        serve_one_widget_request(listener);
        let _ = server_done_tx.send(());
    });

    let script_path = consumer_dir.path().join("smoke.mjs");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ CratestackRuntime, getWidget }} from "node-dist-esm-check/swr";

const runtime = new CratestackRuntime("http://127.0.0.1:{port}", {{ basePath: "/api" }});
const widget = await getWidget(runtime, 1);
if (widget.name !== "Test Widget") {{
  throw new Error(`unexpected widget: ${{JSON.stringify(widget)}}`);
}}
console.log("NODE_DIST_ESM_CHECK_OK");
"#
    )
    .expect("write smoke script");

    // Plain `node`, no `tsx`, no bundler — the exact runtime #315's bug
    // report used (`node -e 'import(...)'`), against a package name
    // resolved through real node_modules, not a relative path into src/.
    let output = Command::new("node")
        .arg("smoke.mjs")
        .current_dir(consumer_dir.path())
        .output()
        .expect("run node smoke.mjs");

    // Bounded wait, not `.join()` — if `node` never connected (import
    // failure), the server thread is still parked in `accept()` and
    // would hang forever; the real pass/fail signal is `output` below.
    let _ = server_done_rx.recv_timeout(std::time::Duration::from_secs(5));

    assert!(
        output.status.success(),
        "generated package's compiled dist/ output failed to import under plain Node ESM \
         (this is the exact regression #315 tracks — extensionless relative specifiers in \
         compiled .js resolve fine under a bundler/tsx but not under node's native ESM \
         resolver):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("NODE_DIST_ESM_CHECK_OK"),
        "smoke script did not print its success marker:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn node_npm_npx_available() -> bool {
    ["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

/// Accepts exactly one HTTP connection, replies to any request with the
/// fixture's `Widget` JSON shape, then returns. Hand-rolled instead of
/// pulling in an HTTP server crate — this only needs to prove one real
/// `fetch()` round-trip. Mirrors `tests/swr_runtime.rs`'s helper of the
/// same shape.
fn serve_one_widget_request(listener: TcpListener) {
    use std::io::{BufRead, BufReader, Write};

    let (stream, _) = listener.accept().expect("accept stub connection");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
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
}
