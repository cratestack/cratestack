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
//! Deliberately covers both the default and `swr` presets — both ship
//! their own `list()`/`{{ list_fn }}` call site wired to
//! `toRpcListInput`, and both share the same generated `src/queries.ts`
//! (see `crate::templates::specs`/`crate::swr::templates`'s module docs
//! for why that file is reused verbatim rather than duplicated).
//!
//! Same Node-availability skip convention as `tests/swr_runtime.rs`:
//! no Rust CI job in this repo currently provisions Node, so this
//! degrades to a printed skip rather than failing a job that was never
//! going to have `node`/`npx` on `PATH`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::Command;

use cratestack_axum::rpc::{RpcListInput, RpcListPredicate};
use cratestack_client_typescript::{TypeScriptGeneratorConfig, TypeScriptPreset, generate_package};

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

    for preset in [TypeScriptPreset::Default, TypeScriptPreset::Swr] {
        assert_generated_wire_shape_matches_rpc_list_input(preset);
    }
}

fn assert_generated_wire_shape_matches_rpc_list_input(preset: TypeScriptPreset) {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/tiny_rpc.cstack")
        .expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "rpc-list-query-wire-check".to_owned(),
            preset,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{preset:?}: package should render: {error}"));

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }
    assert!(
        dir.path().join("src/queries.ts").is_file(),
        "{preset:?}: expected src/queries.ts to be generated for an RPC schema"
    );

    // Same field values as `crates/cratestack-axum/src/rpc/tests_list.rs`'s
    // `synthesize_list_query_round_trips_through_parse_query_pairs`, plus
    // an `or` value (that existing Rust test leaves `or: None`) so this
    // test also covers the one field the Rust-side test doesn't.
    let script_path = dir.path().join("smoke.ts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ toRpcListInput }} from "./src/queries";

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
}});
console.log(JSON.stringify(input));
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
        "{preset:?}: generated toRpcListInput() failed to run under Node:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let actual_json: serde_json::Value = stdout
        .lines()
        .last()
        .and_then(|line| serde_json::from_str(line).ok())
        .unwrap_or_else(|| panic!("{preset:?}: smoke script did not print valid JSON:\n{stdout}"));

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
    };
    let expected_json = serde_json::to_value(&expected_input)
        .expect("serialize the real RpcListInput the server actually decodes");

    assert_eq!(
        actual_json, expected_json,
        "{preset:?}: generated toRpcListInput() output does not match \
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
