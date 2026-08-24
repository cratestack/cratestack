//! Regression coverage for the REST client's `?where=`/`?or=`/arbitrary-
//! predicate query params. `CratestackFetchQuery` used to type `where`,
//! `filters`, and `orFilters` as JSON-ish objects, and `toSearchQuery()`'s
//! `appendQueryValue()` fallback then `JSON.stringify()`'d them into the
//! URL — but the server's list-query grammar
//! (`cratestack_axum::query::FilterExpressionParser`, wired up by
//! `parse_model_list_query` in `cratestack-macros/src/axum/shared_support.rs`)
//! is a flat-text DSL, not JSON: `key=value` predicates joined by `,`
//! (AND) / `|` (OR), with unreserved query params themselves acting as
//! predicates. A caller populating `where`/`filters`/`orFilters` as
//! documented got a hard 400 from the real server. Fixed to mirror the
//! Dart client's convention: `where`/`or` are pre-built DSL strings,
//! `filters` is a flat `Record<string, string>` spread as individual
//! query params.

use std::io::Write as _;

use cratestack_axum::query::{
    QueryExpr, parse_computed_params_object, parse_filter_expression, parse_query_pairs,
};
use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn fetch_query_type_no_longer_json_encodes_where_or_filters() {
    let package = generate_for("tiny_rest", "tiny-rest-client");
    let queries = package_file(&package, "src/queries.ts");

    assert!(
        queries.contains("where?: string;"),
        "CratestackFetchQuery.where must be a plain DSL string, not an object:\n{queries}"
    );
    assert!(
        queries.contains("or?: string;"),
        "CratestackFetchQuery.or must be a plain DSL string:\n{queries}"
    );
    assert!(
        queries.contains("filters?: Record<string, string>;"),
        "CratestackFetchQuery.filters must be a flat key/value predicate map:\n{queries}"
    );
    assert!(
        !queries.contains("orFilters"),
        "orFilters must be gone entirely — replaced by `or`, matching the server's `?or=` \
         key and the Dart client's convention:\n{queries}"
    );

    assert!(
        queries.contains("for (const [key, value] of Object.entries(query.filters ?? {}))"),
        "toSearchQuery() must spread `filters` as individual query params, not nest them \
         under a `filters` key:\n{queries}"
    );
}

/// `toSearchQuery`'s typed `computedParams` surface (`docs/design/computed-fields.md`
/// stage 4) round trips as a single URL-encoded JSON-object query
/// parameter — the same `appendQueryValue` object-value branch `where`/
/// `or`/`filters` deliberately do NOT use (see this file's own header
/// comment for why those are flat DSL strings instead), but which IS the
/// correct wire shape for `?computedParams=` specifically (`docs/design/computed-fields.md`'s
/// REST section: `?computedParams=<url-encoded JSON object>`). Decoded
/// back with the real server-side parser
/// (`cratestack_axum::query::parse_computed_params_object`, the same
/// function every generated model's REST handler calls), not a hand-
/// rolled JSON check.
///
/// Best-effort/skippable — see `rest_list_query_round_trips_through_the_real_server_filter_grammar`'s
/// doc comment for the Node-availability convention.
#[test]
fn computed_params_round_trips_as_a_single_url_encoded_json_object() {
    if !node_and_npx_available() {
        eprintln!(
            "skipping computed_params_round_trips_as_a_single_url_encoded_json_object: \
             `node`/`npx` not on PATH"
        );
        return;
    }

    let package = generate_for("tiny_rest", "tiny-rest-computed-params-check");

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
    let server = std::thread::spawn(move || capture_one_request_line(listener));

    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ TinyRestComputedParamsCheckClient }} from "./src/client";

const client = new TinyRestComputedParamsCheckClient("http://127.0.0.1:{port}", {{ basePath: "/api" }});
await client.widgets.list({{
  query: {{
    computedParams: {{ proxyUrl: {{ width: 800 }} }},
  }},
}});
console.log("REST_COMPUTED_PARAMS_CHECK_OK");
"#
    )
    .expect("write smoke script");

    let output = std::process::Command::new("npx")
        .args(["--yes", "tsx", "smoke.ts"])
        .current_dir(dir.path())
        .output()
        .expect("run npx tsx");

    let request_line = server.join().expect("stub server thread");

    assert!(
        output.status.success(),
        "smoke script failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("REST_COMPUTED_PARAMS_CHECK_OK"),
        "smoke script did not print its success marker:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let raw_query = request_line
        .split_once('?')
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once(" HTTP/"))
        .map(|(query, _)| query)
        .unwrap_or_else(|| panic!("captured request line has no query string: {request_line}"));

    let pairs = parse_query_pairs(Some(raw_query)).expect(
        "the client's query string must parse with the real server-side pair parser \
         (cratestack_axum::parse_query_pairs)",
    );
    let raw_computed_params = pairs
        .iter()
        .find(|(k, _)| k == "computedParams")
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("missing 'computedParams' in parsed query pairs: {pairs:?}"));

    // The real server-side parser, not a hand-rolled JSON check — proves
    // this is genuinely the shape `?computedParams=` handlers decode, not
    // just superficially JSON-shaped text.
    let decoded = parse_computed_params_object(raw_computed_params)
        .expect("the client's computedParams value must parse with the real server-side parser");
    assert_eq!(
        decoded.get("proxyUrl"),
        Some(&serde_json::json!({ "width": 800 })),
        "decoded computedParams did not round-trip the client's value: {decoded:?}"
    );
}

/// The real cross-language round trip: generate the REST client for real,
/// run it under Node against a stub HTTP server, capture the exact raw
/// query string it sends, and feed that string into the real server-side
/// parser (`cratestack-axum`'s `parse_query_pairs` / `parse_filter_expression`)
/// — the same functions `parse_model_list_query` uses. Proves the wire
/// format end to end, not just the generated source text.
///
/// Best-effort/skippable: no Rust CI job in this repo currently provisions
/// Node (see `tests/swr_runtime.rs`), so this degrades to a printed skip
/// rather than failing a job that was never going to have `node`/`npx`.
#[test]
fn rest_list_query_round_trips_through_the_real_server_filter_grammar() {
    if !node_and_npx_available() {
        eprintln!(
            "skipping rest_list_query_round_trips_through_the_real_server_filter_grammar: \
             `node`/`npx` not on PATH"
        );
        return;
    }

    let package = generate_for("tiny_rest", "tiny-rest-client");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    // cratestack#498: `./src/client` (imported below) now imports
    // `./src/models`, which imports `decimal.js` unconditionally (every
    // generated package declares it as a real `dependencies` entry, not
    // just for schemas with a `Decimal` field — see `models.ts.j2`'s doc
    // comment) — so, unlike before #498, this smoke script needs a real
    // `node_modules` to resolve against. Without this, `npx tsx` hangs
    // rather than failing fast (confirmed empirically: `output()` never
    // returns, leaving `capture_one_request_line`'s `listener.accept()`
    // blocked forever with no request ever sent — not investigated
    // further since installing first is the correct fix either way).
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
    let server = std::thread::spawn(move || capture_one_request_line(listener));

    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ TinyRestClientClient }} from "./src/client";

const client = new TinyRestClientClient("http://127.0.0.1:{port}", {{ basePath: "/api" }});
await client.widgets.list({{
  query: {{
    fields: ["id", "name"],
    include: ["owner"],
    includeFields: {{ owner: ["id"] }},
    limit: 5,
    offset: 10,
    where: "published=true,authorId=42",
    or: "role=admin|role=owner",
    filters: {{ status: "active" }},
  }},
}});
console.log("REST_LIST_QUERY_CHECK_OK");
"#
    )
    .expect("write smoke script");

    let output = std::process::Command::new("npx")
        .args(["--yes", "tsx", "smoke.ts"])
        .current_dir(dir.path())
        .output()
        .expect("run npx tsx");

    let request_line = server.join().expect("stub server thread");

    assert!(
        output.status.success(),
        "smoke script failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("REST_LIST_QUERY_CHECK_OK"),
        "smoke script did not print its success marker:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let raw_query = request_line
        .split_once('?')
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once(" HTTP/"))
        .map(|(query, _)| query)
        .unwrap_or_else(|| panic!("captured request line has no query string: {request_line}"));

    assert!(
        !raw_query.contains("%7B") && !raw_query.contains('{'),
        "raw query string must never contain a JSON object — `where`/`or`/`filters` must be \
         sent as flat DSL/key-value pairs, not JSON.stringify'd:\n{raw_query}"
    );

    let pairs = parse_query_pairs(Some(raw_query)).expect(
        "the client's query string must parse with the real server-side pair parser \
         (cratestack_axum::parse_query_pairs)",
    );
    let value_of = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("missing '{key}' in parsed query pairs: {pairs:?}"))
    };

    assert_eq!(value_of("fields"), "id,name");
    assert_eq!(value_of("include"), "owner");
    assert_eq!(value_of("includeFields[owner]"), "id");
    assert_eq!(value_of("limit"), "5");
    assert_eq!(value_of("offset"), "10");
    assert_eq!(value_of("where"), "published=true,authorId=42");
    assert_eq!(value_of("or"), "role=admin|role=owner");
    // `filters` must be spread as an individual top-level param — the exact
    // mechanism `parse_model_list_query` treats as "anything unreserved is
    // a predicate" (`crates/cratestack-macros/src/axum/shared_support.rs`).
    assert_eq!(value_of("status"), "active");

    // The `where` value must be genuinely parseable by the real server
    // grammar, not just present as a string.
    let parsed_where = parse_filter_expression(value_of("where"))
        .expect("the where DSL string the client sent must parse on the server");
    assert_eq!(
        parsed_where,
        QueryExpr::All(vec![
            QueryExpr::Predicate {
                key: "published".to_owned(),
                value: "true".to_owned(),
            },
            QueryExpr::Predicate {
                key: "authorId".to_owned(),
                value: "42".to_owned(),
            },
        ])
    );
}

fn node_and_npx_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
        && std::process::Command::new("npx")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
}

/// Accepts exactly one HTTP connection, records its request line, replies
/// with an empty JSON array (a valid `Widget[]`), then returns the request
/// line to the caller.
fn capture_one_request_line(listener: std::net::TcpListener) -> String {
    use std::io::{BufRead, BufReader, Write};

    let (stream, _) = listener.accept().expect("accept stub connection");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("read request line");
    let request_line = request_line.trim_end().to_owned();

    // Drain the remaining headers.
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read header line");
        if read == 0 || line == "\r\n" {
            break;
        }
    }

    let body = "[]";
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

    request_line
}

fn generate_for(
    fixture_stem: &str,
    package_name: &str,
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
            swr: false,
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
