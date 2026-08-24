//! Round-trip proof for issue #333's typed RPC list-query builder.
//!
//! Compiling is not proof a wire format is correct — this actually runs
//! generated `toRpcListInput()` under Node against a fully-populated
//! `CratestackRpcListQuery`, and asserts the resulting JSON matches
//! `serde_json::to_value` of a real `cratestack_axum::rpc::RpcListInput`
//! built with the same values: the actual struct the server-side RPC
//! dispatch decodes the request body into
//! (`crates/cratestack-macros/src/transport/rpc.rs`'s
//! `decode_rpc_body::<_, RpcListInput>`), not a hand-copied guess at its
//! shape. This is the same struct `crates/cratestack-axum/src/rpc/
//! tests_list.rs`'s `synthesize_list_query_round_trips_through_parse_query_pairs`
//! exercises from the Rust-client side; this test is its TypeScript-side
//! counterpart.
//!
//! Deliberately covers both the default layout and `--swr`'s subtree —
//! both ship their own `list()`/`{{ list_fn }}` call site wired to
//! `toRpcListInput`, and both reuse the same `queries.ts` template
//! verbatim (see `crate::templates::specs`/`crate::swr::templates`'s
//! module docs for why that file is reused rather than duplicated as a
//! distinct template — it lands at `src/queries.ts` for the default
//! layout and `src/swr/queries.ts` for the `--swr` subtree).
//!
//! Same Node-availability skip convention as `tests/swr_runtime.rs`:
//! no Rust CI job in this repo currently provisions Node, so this
//! degrades to a printed skip rather than failing a job that was never
//! going to have `node`/`npx` on `PATH`.
//!
//! Also covers `computedParams` (`docs/design/computed-fields.md`'s typed
//! client computedParams surface — see its "Downstream" section): the TS
//! side passes a plain object
//! (`{ proxyUrl: { width: 800 } }`), and the Rust side asserts against
//! `RpcListInput::computed_params`'s `Option<String>` — the raw
//! `JSON.stringify`d text, not a nested JSON value. Deliberately does NOT
//! `JSON.stringify` on the TS side itself; `toRpcListInput` must do that
//! internally, so a regression that emits the object directly is caught
//! here as a concrete value mismatch (proven by temporarily reverting
//! that stringify call and confirming this test fails with exactly that
//! shape of diff — not committed, see this crate's own report for the
//! transcript).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::Command;

use cratestack_axum::rpc::{RpcListInput, RpcListPredicate};
use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn to_rpc_list_input_matches_the_real_rpc_list_input_wire_shape() {
    if !node_and_npx_available() {
        eprintln!(
            "skipping to_rpc_list_input_matches_the_real_rpc_list_input_wire_shape: \
             `node`/`npx` not on PATH (expected in this repo's Rust-only CI jobs — \
             see this test's module doc)"
        );
        return;
    }

    for swr in [false, true] {
        assert_generated_wire_shape_matches_rpc_list_input(swr);
    }
}

fn assert_generated_wire_shape_matches_rpc_list_input(swr: bool) {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/tiny_rpc.cstack")
        .expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "rpc-list-query-wire-check".to_owned(),
            swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("swr={swr}: package should render: {error}"));

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }
    // The default layout's own `src/queries.ts` is always generated
    // regardless of `swr`; when `swr` is on, its `src/swr/queries.ts`
    // sibling is what this test actually exercises below.
    let queries_relative = if swr {
        "src/swr/queries.ts"
    } else {
        "src/queries.ts"
    };
    assert!(
        dir.path().join(queries_relative).is_file(),
        "swr={swr}: expected {queries_relative} to be generated for an RPC schema"
    );

    // Same field values as `crates/cratestack-axum/src/rpc/tests_list.rs`'s
    // `synthesize_list_query_round_trips_through_parse_query_pairs`, plus
    // an `or` value (that existing Rust test leaves `or: None`) so this
    // test also covers the one field the Rust-side test doesn't.
    let import_specifier = if swr {
        "./src/swr/queries"
    } else {
        "./src/queries"
    };
    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ toRpcListInput }} from "{import_specifier}";

const input = toRpcListInput({{
  limit: 20,
  offset: 40,
  fields: ["id", "title"],
  include: ["author"],
  includeFields: {{ author: ["id", "name"] }},
  sort: "createdAt desc",
  where: "published=true",
  or: "authorId=1|authorId=2",
  filters: [{{ key: "authorId", value: "42" }}],
  // `toRpcListInput` must JSON.stringify this into a raw string, matching
  // `RpcListInput::computed_params`'s `Option<String>` wire shape
  // (`docs/design/computed-fields.md`) — NOT hand `JSON.stringify` it
  // here, so a regression that emits the object directly (rather than
  // stringifying inside `toRpcListInput`) is caught by the mismatch
  // below, not silently matched by this script doing the encoding itself.
  computedParams: {{ proxyUrl: {{ width: 800 }} }},
}});
console.log(JSON.stringify(input));

// An explicit-but-empty `computedParams: {{}}` (no own keys — as opposed
// to a genuinely populated object above) must be omitted from the frame
// entirely, matching REST's `CratestackFetchQuery`/Dart/Rust omission
// behavior for the same shape — not serialized as `computedParams: "{{}}"`.
const emptyInput = toRpcListInput({{ computedParams: {{}} }});
console.log(JSON.stringify(emptyInput));
"#
    )
    .expect("write smoke script");

    let output = Command::new("npx")
        .args(["--yes", "tsx", "smoke.ts"])
        .current_dir(dir.path())
        .output()
        .expect("run npx tsx");
    assert!(
        output.status.success(),
        "swr={swr}: generated toRpcListInput() failed to run under Node:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Two `console.log(JSON.stringify(...))` calls above, in order: the
    // fully-populated input, then the explicit-but-empty-`computedParams`
    // one. Filters rather than indexes by fixed line number because
    // `tsx` can print non-JSON noise (warnings, etc.) ahead of either.
    let json_lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect();
    assert!(
        json_lines.len() >= 2,
        "swr={swr}: smoke script should print two JSON lines (the full input, then the \
         empty-computedParams input):\n{stdout}"
    );
    let actual_json = json_lines[json_lines.len() - 2].clone();
    let empty_computed_params_json = json_lines[json_lines.len() - 1].clone();

    let mut include_fields = BTreeMap::new();
    include_fields.insert(
        "author".to_owned(),
        vec!["id".to_owned(), "name".to_owned()],
    );
    let expected_input = RpcListInput {
        limit: Some(20),
        offset: Some(40),
        fields: Some(vec!["id".to_owned(), "title".to_owned()]),
        include: Some(vec!["author".to_owned()]),
        include_fields,
        sort: Some("createdAt desc".to_owned()),
        where_expr: Some("published=true".to_owned()),
        or: Some("authorId=1|authorId=2".to_owned()),
        filters: vec![RpcListPredicate {
            key: "authorId".to_owned(),
            value: "42".to_owned(),
        }],
        // Must equal `JSON.stringify({ proxyUrl: { width: 800 } })`
        // byte-for-byte — the wire contract is the raw JSON-object TEXT,
        // not a nested object (`RpcListInput::computed_params`'s own doc
        // comment; `docs/design/computed-fields.md`'s "RPC" section).
        computed_params: Some("{\"proxyUrl\":{\"width\":800}}".to_owned()),
    };
    let expected_json = serde_json::to_value(&expected_input)
        .expect("serialize the real RpcListInput the server actually decodes");

    assert!(
        empty_computed_params_json.get("computedParams").is_none(),
        "swr={swr}: toRpcListInput({{ computedParams: {{}} }}) must omit computedParams from \
         the frame entirely — an object with no own keys is not a real params value, matching \
         REST's CratestackFetchQuery/Dart/Rust omission behavior for the same shape — got: \
         {empty_computed_params_json}"
    );

    assert_eq!(
        actual_json, expected_json,
        "swr={swr}: generated toRpcListInput() output does not match \
         serde_json::to_value(&RpcListInput {{ .. }}) — the RPC dispatcher \
         (crates/cratestack-macros/src/transport/rpc.rs) decodes the request \
         body straight into RpcListInput with this exact field set, so any \
         mismatch here (e.g. `includeFields` vs `include_fields`) is a real \
         wire-format bug, not a cosmetic one"
    );
}

fn node_and_npx_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
        && Command::new("npx")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
}
