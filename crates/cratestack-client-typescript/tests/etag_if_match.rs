//! Generated-output coverage for issue #610: the generated TS client's
//! `CratestackRuntime.request()` used to discard the `Response`, which
//! made `ETag`/`If-Match` unreachable even though the generated server
//! requires `If-Match` on PATCH (and, since cratestack#519, DELETE) for
//! any `@version` model.
//!
//! `etag_versioned.cstack`'s `Ledger` model declares `@version` — same
//! shape as `crates/cratestack-client/tests/fixtures/versioned.cstack`,
//! the fixture the *Rust* generated client's own `#493` ETag/If-Match
//! coverage (`crates/cratestack-client/tests/generated_client_versioning.rs`)
//! uses.
//!
//! Two halves, mirroring the issue's own split:
//!   * READ  — `ETag` must be reachable from a `get`/detail response.
//!   * WRITE — `update`/`delete` must accept an optional `ifMatch` and
//!     send it as `If-Match`.
//!
//! `etag_generated_output_round_trips_through_a_real_http_stub_server`
//! at the bottom is the real, Node-driven proof (skips, printed, when
//! `node`/`npm`/`npx` aren't on `PATH` — same convention as
//! `tests/rest_list_query_wire_format.rs` and `tests/swr_runtime.rs`).

use std::io::Write as _;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn versioned_model_update_and_delete_accept_if_match_and_send_the_header() {
    let package = generate_for("etag_versioned", "etag-versioned-client");
    let client = package_file(&package, "src/client.ts");

    assert!(
        client.contains(
            "update(\n    id: number,\n    input: UpdateLedgerInput,\n    options: CratestackWriteRequestConfig = {},\n  ): Promise<Ledger>"
        ),
        "update() must accept an optional ifMatch via CratestackWriteRequestConfig:\n{client}"
    );
    assert!(
        client.contains("headers: withIfMatchHeader(options.headers, options.ifMatch),"),
        "update()/delete() must translate options.ifMatch into an If-Match header via \
         withIfMatchHeader:\n{client}"
    );
    assert!(
        client.contains(
            "delete(id: number, options: CratestackWriteRequestConfig = {}): Promise<void>"
        ),
        "delete() must also accept an optional ifMatch (DELETE on an @version model requires \
         If-Match since cratestack#519):\n{client}"
    );
    // Both call sites (update + delete) must go through the same helper.
    assert_eq!(
        client
            .matches("headers: withIfMatchHeader(options.headers, options.ifMatch),")
            .count(),
        2,
        "both update() and delete() must merge ifMatch into the If-Match header:\n{client}"
    );

    let queries = package_file(&package, "src/queries.ts");
    assert!(
        queries.contains(
            "export interface CratestackWriteRequestConfig extends CratestackRequestConfig {"
        ),
        "queries.ts must define CratestackWriteRequestConfig with an ifMatch field:\n{queries}"
    );
    assert!(
        queries.contains("ifMatch?: string;"),
        "CratestackWriteRequestConfig.ifMatch must be optional:\n{queries}"
    );
    assert!(
        queries.contains("export function withIfMatchHeader("),
        "queries.ts must export the withIfMatchHeader helper:\n{queries}"
    );
}

#[test]
fn versioned_model_get_response_reaches_the_etag_header() {
    let package = generate_for("etag_versioned", "etag-versioned-client");
    let client = package_file(&package, "src/client.ts");
    let runtime = package_file(&package, "src/runtime.ts");

    assert!(
        client.contains(
            "getWithResponse(\n    id: number,\n    options: CratestackQueryRequestConfig = {},\n  ): Promise<CratestackResponseEnvelope<Ledger>>"
        ),
        "getWithResponse() must exist and return the response alongside the record:\n{client}"
    );
    assert!(
        client.contains("return this.runtime.getWithResponse<unknown>("),
        "getWithResponse() must call through to the runtime's response-preserving method:\n{client}"
    );
    assert!(
        client.contains("response: result.response,"),
        "getWithResponse() must surface the raw Response object (so a caller can read \
         response.headers.get(\"etag\")):\n{client}"
    );

    assert!(
        runtime.contains("export interface CratestackResponseEnvelope<T>"),
        "runtime.ts must export CratestackResponseEnvelope:\n{runtime}"
    );
    assert!(
        runtime.contains("async requestWithResponse<T>("),
        "runtime.ts must define requestWithResponse, the response-preserving primitive:\n{runtime}"
    );
    assert!(
        runtime.contains("getWithResponse<T>("),
        "runtime.ts must expose a getWithResponse convenience method:\n{runtime}"
    );
    // request() must still exist and keep its old (body-only) return shape,
    // so this is additive, not a breaking rename.
    assert!(
        runtime.contains("async request<T>(") && runtime.contains("return value;"),
        "request() must remain a thin wrapper that discards nothing callers already relied \
         on — its return type is unchanged:\n{runtime}"
    );
}

/// The `swr` preset's plain per-model functions (`src/swr/*.ts`) get the
/// same WRITE-side `ifMatch` treatment as the default `client.ts` — the
/// two are separate, hand-maintained templates (issue #591's additive
/// `--swr` file set), so this is not implied by the tests above.
#[test]
fn swr_preset_update_and_delete_functions_also_accept_if_match() {
    let package = generate_for_swr("etag_versioned", "etag-versioned-swr-client");
    let model_file = package_file(&package, "src/swr/models/ledger.ts");

    assert!(
        model_file.contains("options: CratestackWriteRequestConfig = {},"),
        "swr's updateLedger/deleteLedger must accept CratestackWriteRequestConfig:\n{model_file}"
    );
    assert!(
        model_file.contains("headers: withIfMatchHeader(options.headers, options.ifMatch),"),
        "swr's updateLedger/deleteLedger must merge ifMatch into an If-Match header:\n{model_file}"
    );
    assert_eq!(
        model_file
            .matches("headers: withIfMatchHeader(options.headers, options.ifMatch),")
            .count(),
        2,
        "both the update and delete plain functions must apply the fix:\n{model_file}"
    );
}

/// Real, Node-driven proof of the full round trip this issue is about:
/// GET a versioned record through the generated client, read `ETag` off
/// `getWithResponse`'s `response`, PATCH with that value as `ifMatch`,
/// and confirm the raw HTTP request the generated client actually sent
/// carried a real `If-Match` header with the right value.
#[test]
fn etag_generated_output_round_trips_through_a_real_http_stub_server() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping etag_generated_output_round_trips_through_a_real_http_stub_server: \
             `node`/`npm`/`npx` not on PATH (expected in this repo's Rust-only CI jobs)"
        );
        return;
    }

    let package = generate_for("etag_versioned", "etag-versioned-client");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    let install = std::process::Command::new("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path())
        .output()
        .expect("run npm install");
    assert!(
        install.status.success(),
        "npm install failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub server");
    let port = listener.local_addr().expect("local addr").port();
    let server = std::thread::spawn(move || run_etag_stub_server(listener));

    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ EtagVersionedClientClient }} from "./src/client";

const client = new EtagVersionedClientClient("http://127.0.0.1:{port}", {{ basePath: "/api" }});

const got = await client.ledgers.getWithResponse(4);
const etag = got.response.headers.get("etag");
if (etag === null) {{
  throw new Error("no etag header reached the caller");
}}

const updated = await client.ledgers.update(
  4,
  {{ balance: 5 }},
  {{ ifMatch: etag }},
);
if (updated.balance !== 5) {{
  throw new Error("update did not round-trip the new balance");
}}

console.log("ETAG_IF_MATCH_CHECK_OK");
"#
    )
    .expect("write smoke script");

    let output = std::process::Command::new("npx")
        .args(["--yes", "tsx", "smoke.ts"])
        .current_dir(dir.path())
        .output()
        .expect("run npx tsx");

    let captured = server.join().expect("stub server thread");

    assert!(
        output.status.success(),
        "smoke script failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ETAG_IF_MATCH_CHECK_OK"),
        "smoke script did not print its success marker:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        captured.get_request_line.starts_with("GET /api/ledgers/4"),
        "expected a GET on the detail route first: {}",
        captured.get_request_line
    );
    assert!(
        captured
            .patch_request_line
            .starts_with("PATCH /api/ledgers/4"),
        "expected a PATCH on the detail route second: {}",
        captured.patch_request_line
    );
    assert_eq!(
        captured.patch_if_match_header.as_deref(),
        Some("\"7\""),
        "the PATCH request must carry the If-Match header the client learned from the GET's \
         ETag — this is the exact round trip issue #610 says the generated client couldn't do"
    );
}

fn node_npm_npx_available() -> bool {
    ["node", "npm", "npx"].iter().all(|cmd| {
        std::process::Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

struct CapturedRequests {
    get_request_line: String,
    patch_request_line: String,
    patch_if_match_header: Option<String>,
}

/// Accepts exactly two HTTP connections: a GET (replies with a Ledger
/// body and an `ETag: "7"` header) then a PATCH (records whatever
/// `If-Match` header the client sent, replies with the updated Ledger).
fn run_etag_stub_server(listener: std::net::TcpListener) -> CapturedRequests {
    use std::io::{BufRead, BufReader, Read, Write};

    let get_request_line = handle_one_request(&listener, |request_line, _headers| {
        let body = r#"{"id":4,"label":"gl-4","balance":1,"version":7}"#;
        (
            request_line,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"7\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            ),
        )
    });

    let (patch_request_line, if_match) = handle_one_request(&listener, |request_line, headers| {
        let if_match = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("if-match"))
            .map(|(_, value)| value.clone());
        let body = r#"{"id":4,"label":"gl-4","balance":5,"version":8}"#;
        (
            (request_line, if_match),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"8\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            ),
        )
    });

    return CapturedRequests {
        get_request_line,
        patch_request_line,
        patch_if_match_header: if_match,
    };

    fn handle_one_request<T>(
        listener: &std::net::TcpListener,
        respond: impl FnOnce(String, Vec<(String, String)>) -> (T, String),
    ) -> T {
        let (stream, _) = listener.accept().expect("accept stub connection");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));

        let mut request_line = String::new();
        reader
            .read_line(&mut request_line)
            .expect("read request line");
        let request_line = request_line.trim_end().to_owned();

        let mut headers = Vec::new();
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).expect("read header line");
            if read == 0 || line == "\r\n" {
                break;
            }
            let line = line.trim_end().to_owned();
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_owned();
                let value = value.trim().to_owned();
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.push((name, value));
            }
        }
        if content_length > 0 {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body).expect("read request body");
        }

        let (result, response) = respond(request_line, headers);

        let mut stream = stream;
        stream
            .write_all(response.as_bytes())
            .expect("write stub response");
        stream.flush().expect("flush stub response");

        result
    }
}

fn generate_for(
    fixture_stem: &str,
    package_name: &str,
) -> cratestack_client_typescript::GeneratedTypeScriptPackage {
    generate_with_config(fixture_stem, package_name, false)
}

fn generate_for_swr(
    fixture_stem: &str,
    package_name: &str,
) -> cratestack_client_typescript::GeneratedTypeScriptPackage {
    generate_with_config(fixture_stem, package_name, true)
}

fn generate_with_config(
    fixture_stem: &str,
    package_name: &str,
    swr: bool,
) -> cratestack_client_typescript::GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            swr,
            full_selection: false,
            refine: false,
            tanstack: false,
            schema_sha256: String::new(),
        },
    )
    .expect("default template should render")
}

fn package_file<'a>(
    package: &'a cratestack_client_typescript::GeneratedTypeScriptPackage,
    file_name: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}
