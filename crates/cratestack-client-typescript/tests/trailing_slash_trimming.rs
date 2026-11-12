//! Regression tests for issue #1236: generated TypeScript runtimes must not
//! trim trailing slashes with the polynomial `/\/+$/` regex (CodeQL
//! `js/polynomial-redos`).

use std::fs;
use std::path::Path;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const REST_SCHEMA: &str =
    "model Widget {\n  id Int @id\n  name String\n  @@allow(\"read\", auth() != null)\n}\n";

fn write_package(dir: &Path, schema_source: &str, swr: bool) {
    let schema = cratestack_parser::parse_schema(schema_source).expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("package should generate");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
}

#[test]
fn generated_typescript_packages_contain_no_polynomial_trailing_slash_regex() {
    let sub_schema =
        fs::read_to_string("tests/fixtures/subscriptions.cstack").expect("read subscriptions");
    let cases = [
        (
            "rest_full",
            REST_SCHEMA.to_owned(),
            &[
                "src/runtime.ts",
                "src/swr/runtime.ts",
                "src/compact.ts",
                "src/swr/compact.ts",
            ][..],
        ),
        (
            "rpc_full",
            format!("transport rpc\n{REST_SCHEMA}"),
            &["src/runtime.ts", "src/swr/runtime.ts"][..],
        ),
        (
            "subscriptions",
            sub_schema,
            &[
                "src/runtime.ts",
                "src/swr/runtime.ts",
                "src/subscriptions.ts",
            ][..],
        ),
    ];

    for (label, source, helper_files) in cases {
        let schema = cratestack_parser::parse_schema(&source).expect("parses");
        let package = generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                swr: true,
                tanstack: true,
                refine: true,
                rtk: true,
                ..TypeScriptGeneratorConfig::default()
            },
        )
        .expect("renders");

        for file in &package.files {
            assert!(
                !file.contents.contains("/\\/+$/"),
                "{label}: {} still contains polynomial regex /\\/+$/",
                file.file_name
            );
        }
        for expected_file in helper_files {
            let contents = &package
                .files
                .iter()
                .find(|file| file.file_name == *expected_file)
                .unwrap_or_else(|| panic!("{label}: missing {expected_file}"))
                .contents;
            assert!(
                contents.contains("function trimTrailingSlashes(value: string): string"),
                "{label}: {expected_file} missing trimTrailingSlashes helper"
            );
        }
    }
}

#[test]
fn generated_runtimes_and_helpers_trim_trailing_slashes_in_linear_time() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping generated_runtimes_and_helpers_trim_trailing_slashes_in_linear_time: no node/npm"
        );
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"trailing-slash-check","private":true,"type":"module","dependencies":{"decimal.js":"^10.6.0"}}"#,
    )
    .expect("write package.json");
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
    let cbor_stub_dir = dir.path().join("node_modules/@cratestack/cbor");
    fs::create_dir_all(&cbor_stub_dir).expect("create @cratestack/cbor stub dir");
    fs::write(
        cbor_stub_dir.join("package.json"),
        r#"{"name":"@cratestack/cbor","version":"0.0.0","type":"module","exports":"./index.js"}"#,
    )
    .expect("write stub package.json");
    fs::write(
        cbor_stub_dir.join("index.js"),
        "export async function createCborCodec() { throw new Error('unused'); }\nexport async function createCompactKeys() { throw new Error('unused'); }\n",
    )
    .expect("write stub index.js");

    let rest_dir = dir.path().join("rest");
    let rpc_dir = dir.path().join("rpc");
    let sub_dir = dir.path().join("sub");

    let sub_schema =
        fs::read_to_string("tests/fixtures/subscriptions.cstack").expect("read subscriptions");
    write_package(&rest_dir, REST_SCHEMA, true);
    write_package(&rpc_dir, &format!("transport rpc\n{REST_SCHEMA}"), true);
    write_package(&sub_dir, &sub_schema, false);

    let script = r#"
import assert from "node:assert/strict";
import { CratestackRuntime as RestRuntime } from "./rest/src/runtime.ts";
import { CratestackRuntime as SwrRestRuntime } from "./rest/src/swr/runtime.ts";
import { RestCompactSession } from "./rest/src/compact.ts";
import { CratestackRpcRuntime as RpcRuntime } from "./rpc/src/runtime.ts";
import { CratestackRpcRuntime as SwrRpcRuntime } from "./rpc/src/swr/runtime.ts";
import { resolveUrl } from "./sub/src/subscriptions.ts";

async function main() {
  for (const Runtime of [RestRuntime, SwrRestRuntime, RpcRuntime, SwrRpcRuntime]) {
    assert.equal(new Runtime("https://api.example.com").origin, "https://api.example.com");
    assert.equal(new Runtime("https://api.example.com/").origin, "https://api.example.com");
    assert.equal(new Runtime("https://api.example.com///").origin, "https://api.example.com");
    assert.equal(new Runtime("///").origin, "");
  }

  const restUrls = async (Runtime: typeof RestRuntime) => {
    const seen: string[] = [];
    const fetchFn: typeof fetch = async (input) => {
      seen.push(String(input));
      return new Response("{}", { headers: { "content-type": "application/json" } });
    };
    for (const basePath of ["", "/", "///", "/api", "/api/", "/api///", "/a/b///"]) {
      await new Runtime("https://api.example.com///", { basePath, fetch: fetchFn }).get("/widgets");
    }
    assert.deepEqual(seen, [
      "https://api.example.com/widgets",
      "https://api.example.com/widgets",
      "https://api.example.com/widgets",
      "https://api.example.com/api/widgets",
      "https://api.example.com/api/widgets",
      "https://api.example.com/api/widgets",
      "https://api.example.com/a/b/widgets",
    ]);
  };
  await restUrls(RestRuntime);
  await restUrls(SwrRestRuntime);

  for (const Runtime of [RpcRuntime, SwrRpcRuntime]) {
    const seen: string[] = [];
    const fetchFn: typeof fetch = async (input) => {
      seen.push(String(input));
      return new Response(new Uint8Array([0xf6]), { headers: { "content-type": "application/cbor" } });
    };
    const codec = { contentType: "application/cbor", encode: () => new Uint8Array([0xf6]), decode: () => null };
    for (const basePath of ["", "/", "///", "/api///"]) {
      await new Runtime("https://api.example.com///", { basePath, fetch: fetchFn, codec }).call("model.Widget.list");
    }
    assert.deepEqual(seen, [
      "https://api.example.com/rpc/model.Widget.list",
      "https://api.example.com/rpc/model.Widget.list",
      "https://api.example.com/rpc/model.Widget.list",
      "https://api.example.com/api/rpc/model.Widget.list",
    ]);
  }

  const compact = new RestCompactSession("", ["GET /widgets/{id}"]);
  assert.equal(compact.opOf("GET", "/widgets/42///?a=1"), "GET /widgets/{id}");

  assert.equal(resolveUrl({ origin: "https://api.example.com", basePath: "/", fetchFn: fetch, defaultHeaders: undefined }, "/rpc/ws"), "https://api.example.com/rpc/ws");
  assert.equal(resolveUrl({ origin: "https://api.example.com", basePath: "/api///", fetchFn: fetch, defaultHeaders: undefined }, "/rpc/ws"), "https://api.example.com/api/rpc/ws");

  const pathological = "/x" + "/".repeat(100_000) + "a";
  const started = performance.now();
  assert.equal(new RestRuntime(`https://api.example.com${pathological}`).origin, `https://api.example.com${pathological}`);
  assert.equal(resolveUrl({ origin: "https://api.example.com", basePath: pathological, fetchFn: fetch, defaultHeaders: undefined }, "/ws"), `https://api.example.com${pathological}/ws`);
  assert.equal(compact.opOf("GET", pathological), undefined);
  assert.ok(performance.now() - started < 200, "trimTrailingSlashes must run in linear time");
  console.log("TRAILING_SLASH_CHECK_OK");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
"#;

    fs::write(dir.path().join("verify.ts"), script).expect("write verify script");
    let mut tsx = tsx_command(dir.path(), "verify.ts");
    let output = tsx.output().expect("run tsx");
    assert!(
        output.status.success(),
        "trailing-slash verification script failed:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("TRAILING_SLASH_CHECK_OK"),
        "missing success marker:\n{}",
        command_report(&tsx, &output)
    );
}
