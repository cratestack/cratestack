//! Real npm/vitest proof that a `BigInt` (ADR 0019, a `bigint` in the generated
//! TypeScript client) is usable as a cache-key part in every preset: the
//! TanStack Query keys, the RTK Query endpoints, the SWR keys and refine. The
//! generated text alone cannot show this, because every failure here is a
//! runtime `TypeError: Do not know how to serialize a BigInt` from somebody
//! else's `JSON.stringify`.
//!
//! One package per transport is generated from `tests/fixtures/bigint_query_keys.cstack`
//! (the RPC one by adding `transport rpc`) with `--tanstack --rtk --swr`, then:
//!
//! 1. `npm install` from the generated manifest plus the test devDependencies,
//!    so the declared peer ranges are the ones resolved;
//! 2. `npm run build` (`tsc`), which is what proves the generated RTK endpoints
//!    typecheck with a `bigint` key (RTK tag ids are `string | number`);
//! 3. every suite in `tests/js/bigint_query_keys/`:
//!    `tanstack.test.tsx`, `rtk.test.tsx`, `swr.test.tsx`, `refine.test.tsx`;
//! 4. `@reduxjs/toolkit` re-pinned to exactly the floor the generated manifest
//!    declares (with React 18, the newest React that floor's peer range
//!    allows), the package built again and `rtk.test.tsx` run again. RTK's
//!    default `serializeQueryArgs` throws on a `bigint` before 2.2.4, and
//!    `rtk-api.ts` does not emit declarations before 2.2.7 (TS2527), so a floor
//!    set too low fails here.
//!
//! `--refine` is not generated: its manifest depends on `@cratestack/refine`
//! from npm, and the provider under test is this repository's own
//! `packages/cratestack-refine/src`, copied in as `refine-pkg/`.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't on
//! `PATH`, the same convention as every Node-driven test in this crate; CI runs
//! it because `ubuntu-latest` ships Node.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};
use serde_json::Value;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bigint_query_keys.cstack"
);
const JS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/bigint_query_keys");
const REFINE_SRC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/cratestack-refine/src"
);

/// Copied next to the generated package; `harness.ts` is imported by the rest.
const JS_ASSETS: [&str; 6] = [
    "vitest.config.ts",
    "harness.ts",
    "tanstack.test.tsx",
    "rtk.test.tsx",
    "swr.test.tsx",
    "refine.test.tsx",
];
const SUITES: usize = 4;

/// What the suites render and run with, on top of the generated manifest.
const TEST_DEV_DEPENDENCIES: [(&str, &str); 6] = [
    ("vitest", "^4.1.10"),
    ("jsdom", "^30.0.1"),
    ("@testing-library/react", "^16.0.0"),
    ("react-dom", "^19.0.0"),
    ("@types/react-dom", "^19.0.0"),
    ("@refinedev/core", "^5.0.0"),
];

#[test]
fn rest_presets_accept_a_bigint_in_every_cache_key() {
    run("rest_presets_accept_a_bigint_in_every_cache_key", false);
}

#[test]
fn rpc_presets_accept_a_bigint_in_every_cache_key() {
    run("rpc_presets_accept_a_bigint_in_every_cache_key", true);
}

fn run(test_name: &str, rpc: bool) {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping {test_name}: `node`/`npm`/`npx` not on PATH (expected only where Node is \
             absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }
    let transport = if rpc { "rpc" } else { "rest" };
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = generate_into(dir.path(), rpc);

    let install = npm(dir.path(), &["install", "--no-audit", "--no-fund"]);
    assert_ok(&install, &format!("{test_name}: npm install"));

    let build = npm(dir.path(), &["run", "build"]);
    assert_ok(
        &build,
        &format!(
            "{test_name}: npm run build (tsc) of the generated {transport} package with a \
             BigInt @id under --tanstack --rtk --swr"
        ),
    );

    // Every suite, against the newest `@reduxjs/toolkit` 2.x the range allows.
    let all = vitest(dir.path(), transport, &[]);
    assert_passed(&all, SUITES, &JS_ASSETS[2..], test_name);

    // The floor: pin the toolkit to exactly what the manifest declares and run
    // the RTK suite again. React goes to 18 first, in its own step: the
    // toolkit's `react` peer range only gains `^19` in 2.5.0, so the floor next to
    // the React 19 the install above resolved is an ERESOLVE (and npm refuses
    // the two as one command).
    let react18 = npm(
        dir.path(),
        &[
            "install",
            "--no-audit",
            "--no-fund",
            "react@^18",
            "react-dom@^18",
            "@types/react@^18",
            "@types/react-dom@^18",
        ],
    );
    assert_ok(&react18, &format!("{test_name}: pin react@18"));
    let floor = declared_toolkit_floor(&manifest);
    let pin = npm(
        dir.path(),
        &[
            "install",
            "--no-audit",
            "--no-fund",
            "--save-exact",
            &format!("@reduxjs/toolkit@{floor}"),
        ],
    );
    assert_ok(&pin, &format!("{test_name}: pin @reduxjs/toolkit@{floor}"));
    let installed = fs::read_to_string(
        dir.path()
            .join("node_modules/@reduxjs/toolkit/package.json"),
    )
    .expect("read the installed @reduxjs/toolkit manifest");
    assert!(
        installed.contains(&format!("\"version\": \"{floor}\"")),
        "{test_name}: expected @reduxjs/toolkit {floor} to be installed:\n{installed}"
    );
    let build_at_floor = npm(dir.path(), &["run", "build"]);
    assert_ok(
        &build_at_floor,
        &format!("{test_name}: npm run build (tsc) at @reduxjs/toolkit {floor}"),
    );
    let at_floor = vitest(dir.path(), transport, &["rtk.test.tsx"]);
    assert_passed(
        &at_floor,
        1,
        &["rtk.test.tsx"],
        &format!("{test_name} at the floor {floor}"),
    );
}

/// Writes the generated package, the suites and the refine provider source into
/// `dir`; returns the merged `package.json`.
fn generate_into(dir: &Path, rpc: bool) -> Value {
    let source = fs::read_to_string(FIXTURE).expect("read the fixture");
    let source = if rpc {
        source.replacen("model Counter {", "transport rpc\n\nmodel Counter {", 1)
    } else {
        source
    };
    let schema = cratestack_parser::parse_schema(&source).expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "bigint-query-keys".to_owned(),
            tanstack: true,
            rtk: true,
            swr: true,
            // The pure-TypeScript codec: the native one is
            // `tests/native_cbor_bigint_encode.rs`, and it would add an npm
            // dependency to a suite that is about cache keys.
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--tanstack --rtk --swr should render");

    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    // Keep the generated manifest (its peer ranges are what is under test) and
    // add only what the suites need to run.
    let manifest_path = dir.join("package.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read package.json"))
            .expect("the generated package.json is JSON");
    let dev = manifest["devDependencies"]
        .as_object_mut()
        .expect("devDependencies is an object");
    for (name, range) in TEST_DEV_DEPENDENCIES {
        dev.insert(name.to_owned(), Value::String(range.to_owned()));
    }
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("serialize"),
    )
    .expect("write package.json");

    for asset in JS_ASSETS {
        fs::copy(format!("{JS_DIR}/{asset}"), dir.join(asset))
            .unwrap_or_else(|error| panic!("copy {asset} into the generated package: {error}"));
    }
    copy_ts_files(Path::new(REFINE_SRC), &dir.join("refine-pkg"));
    manifest
}

/// `@reduxjs/toolkit`'s declared floor: the version in `^X.Y.Z`.
fn declared_toolkit_floor(manifest: &Value) -> String {
    let range = manifest["devDependencies"]["@reduxjs/toolkit"]
        .as_str()
        .expect("the generated manifest declares @reduxjs/toolkit under --rtk");
    assert_eq!(
        manifest["peerDependencies"]["@reduxjs/toolkit"], range,
        "the peer and dev ranges of @reduxjs/toolkit should agree"
    );
    range
        .strip_prefix('^')
        .unwrap_or_else(|| panic!("expected a caret range for @reduxjs/toolkit, got {range:?}"))
        .to_owned()
}

fn vitest(dir: &Path, transport: &str, files: &[&str]) -> Output {
    Command::new("npx")
        .args(["--yes", "vitest", "run", "--reporter=verbose"])
        .args(files)
        .env("B8_TRANSPORT", transport)
        .current_dir(dir)
        .output()
        .expect("run npx vitest")
}

fn npm(dir: &Path, args: &[&str]) -> Output {
    Command::new("npm")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run npm")
}

fn assert_ok(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A successful exit alone cannot tell a green run from an accidentally empty
/// one: the verbose reporter names every file it ran, and a skipped or todo
/// test would show up in the summary.
fn assert_passed(output: &Output, files: usize, names: &[&str], what: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "vitest failed ({what}):\nstdout: {stdout}\nstderr: {stderr}"
    );
    for name in names {
        assert!(
            stdout.contains(name),
            "vitest did not report running {name} ({what}):\nstdout: {stdout}"
        );
    }
    assert!(
        stdout.contains(&format!("Test Files  {files} passed ({files})"))
            && !stdout.contains("skipped")
            && !stdout.contains("todo"),
        "vitest did not pass all {files} file(s) cleanly ({what}):\nstdout: {stdout}"
    );
}

/// Copies the `.ts` files of `from` (no subdirectories needed) into `to`.
fn copy_ts_files(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create refine-pkg dir");
    for entry in fs::read_dir(from).expect("read refine src") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "ts") {
            fs::copy(&path, to.join(path.file_name().expect("file name")))
                .expect("copy refine src");
        }
    }
}

fn node_npm_npx_available() -> bool {
    ["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
