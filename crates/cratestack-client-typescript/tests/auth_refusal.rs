//! cratestack#1243: an `AuthProvider` refusal reaches the generated
//! TypeScript client typed, on both transports: the fixed `code`, `details`,
//! every extra member of the body (never dropped) and the
//! `WWW-Authenticate` challenges.
//!
//! The structural tests always run; the Node-driven one sends a real call to
//! a stub server answering a refusal shaped exactly as the server writes it
//! (skips, printed, when `node`/`npm` are not on `PATH`, like
//! `etag_if_match.rs`).

use std::io::{BufRead, BufReader, Read, Write};

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

fn generate(fixture: &str, package_name: &str) -> GeneratedTypeScriptPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            swr: false,
            full_selection: false,
            refine: false,
            tanstack: false,
            schema_sha256: String::new(),
            // JSON on the wire, so the stub below answers in JSON.
            native_cbor: false,
            rtk: false,
        },
    )
    .expect("default template should render")
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("missing generated file {name}"))
        .contents
        .as_str()
}

#[test]
fn both_error_classes_type_the_refusal_members_and_the_challenges() {
    let rest = generate("tiny_rest", "tiny-rest-client");
    let runtime = file(&rest, "src/runtime.ts");
    for needle in [
        "export interface CratestackErrorResponse {",
        "readonly body: CratestackErrorResponse | undefined;",
        "this.extra = extraErrorMembers(this.body);",
        "this.challenges = challengesOf(response.headers);",
    ] {
        assert!(runtime.contains(needle), "REST runtime lacks `{needle}`");
    }

    let rpc = generate("tiny_rpc", "tiny-rpc-client");
    let runtime = file(&rpc, "src/runtime.ts");
    for needle in [
        "constructor(status: number, body: RpcErrorBody, headers?: Headers) {",
        "this.extra = extraErrorMembers(body);",
        "this.challenges = challengesOf(headers);",
        "      await readErrorBody(response, codec),\n      response.headers,\n",
    ] {
        assert!(runtime.contains(needle), "RPC runtime lacks `{needle}`");
    }
}

/// The refusal both stubs answer with: two `WWW-Authenticate` lines, a
/// `details` member and two extra members.
fn refusal_response(code: &str) -> String {
    let body = format!(
        r#"{{"code":"{code}","message":"attestation expired","details":{{"device":"d-1"}},"error":"attestation_expired","retry":{{"after":3}}}}"#
    );
    format!(
        "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\
         WWW-Authenticate: Signature error=\"attestation_expired\"\r\n\
         WWW-Authenticate: Bearer realm=\"api\"\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Answers exactly one request with `response`, then returns.
fn answer_once(listener: &std::net::TcpListener, response: &str) {
    let (stream, _) = listener.accept().expect("accept");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).expect("read line") == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).expect("read body");
    let mut stream = stream;
    stream.write_all(response.as_bytes()).expect("write");
    stream.flush().expect("flush");
}

const EXPECTED: &str = r#"{"code":"CODE","details":{"device":"d-1"},"extra":{"error":"attestation_expired","retry":{"after":3}},"challenges":["Signature error=\"attestation_expired\", Bearer realm=\"api\""]}"#;

fn run_against_stub(fixture: &str, package_name: &str, class: &str, code: &str) {
    let package = generate(fixture, package_name);
    let dir = tempfile::tempdir().expect("tempdir");
    for generated in &package.files {
        let path = dir.path().join(&generated.file_name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, &generated.contents).expect("write");
    }
    let mut install = std::process::Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed:\n{}",
        command_report(&install, &installed)
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let response = refusal_response(code);
    let server = std::thread::spawn(move || answer_once(&listener, &response));

    std::fs::write(
        dir.path().join("smoke.ts"),
        format!(
            r#"
import {{ {class} }} from "./src/client";

const client = new {class}("http://127.0.0.1:{port}", {{ basePath: "/api" }});
try {{
  await client.procedures.echoName({{ name: "x" }});
  throw new Error("the call did not fail");
}} catch (error: any) {{
  console.log("REFUSAL " + JSON.stringify({{
    code: error.code,
    details: error.details,
    extra: error.extra,
    challenges: error.challenges,
  }}));
}}
"#
        ),
    )
    .expect("write smoke script");
    let mut tsx = tsx_command(dir.path(), "smoke.ts");
    let output = tsx.output().expect("run tsx");
    // Assert before joining: a script that died before connecting would
    // otherwise leave the stub parked in `accept()` (see etag_if_match.rs).
    assert!(
        output.status.success(),
        "smoke script failed:\n{}",
        command_report(&tsx, &output)
    );
    server.join().expect("stub server");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("REFUSAL "))
        .unwrap_or_else(|| panic!("no refusal line:\n{}", command_report(&tsx, &output)));
    assert_eq!(line, EXPECTED.replace("CODE", code));
}

#[test]
fn a_refusal_reaches_both_generated_clients_typed() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping a_refusal_reaches_both_generated_clients_typed: `node`/`npm` not on PATH"
        );
        return;
    }
    run_against_stub(
        "tiny_rest",
        "tiny-rest-client",
        "TinyRestClientClient",
        "UNAUTHORIZED",
    );
    run_against_stub(
        "tiny_rpc",
        "tiny-rpc-client",
        "TinyRpcClientClient",
        "unauthenticated",
    );
}
