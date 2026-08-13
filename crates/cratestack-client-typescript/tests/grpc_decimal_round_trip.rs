//! Real `npm install` + `npx vitest run` proof that `transport grpc`'s
//! generated `Decimal` handling (cratestack#498/#499 F4) actually decodes
//! into a real `Decimal` instance at runtime, not a generated-text
//! assertion. Mirrors `tests/decimal_round_trip.rs`'s pattern: generate a
//! real package from an inline `transport grpc` schema with a `Decimal`
//! field, drop a real vitest suite alongside it that exercises the
//! generated `runtime.ts`'s exported `encodeMessage`/`decodeMessage`
//! against a hand-built field-descriptor table (the same shape
//! `grpc-web-client.ts.j2`'s generated `MESSAGES` object would produce —
//! not exported itself, since it's client-internal, so the descriptor is
//! reconstructed here rather than imported), proving the new `"decimal"`
//! `GrpcWireKind` round-trips a scientific-notation value exactly through
//! real generated code.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't
//! on `PATH` — same rationale as `tests/decimal_round_trip.rs`.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, TypeScriptPreset, generate_package};

const SCHEMA: &str = r#"
transport grpc

datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Invoice {
  id Int @id
  amountXaf Decimal

  @@allow("read", auth() != null)
}
"#;

const CHECK_TEST: &str = r#"
import { describe, expect, it } from "vitest";
import { Decimal } from "./src/models.js";
import { encodeMessage, decodeMessage, type GrpcFieldDescriptor } from "./src/runtime.js";

// Mirrors the shape `grpc-web-client.ts.j2`'s generated (but
// client-internal, non-exported) `MESSAGES.Invoice` descriptor would have
// for this schema's `Invoice` model — field number doesn't need to match
// a real `.pb.lock` entry for this test, since encode and decode both run
// against the identical table.
const INVOICE_FIELDS: GrpcFieldDescriptor[] = [
  { property: "id", number: 1, kind: "int64", repeated: false },
  { property: "amountXaf", number: 2, kind: "decimal", repeated: false },
];

describe("gRPC-Web Decimal wire kind (cratestack#499 F4)", () => {
  it("round-trips a scientific-notation value through encodeMessage/decodeMessage as a real Decimal", () => {
    const value = { id: 1, amountXaf: new Decimal("1E-7") };
    const bytes = encodeMessage(value, INVOICE_FIELDS, {}, {});
    const decoded = decodeMessage(bytes, INVOICE_FIELDS, {}, {});

    expect(decoded.amountXaf).toBeInstanceOf(Decimal);
    expect((decoded.amountXaf as InstanceType<typeof Decimal>).equals(new Decimal("0.0000001"))).toBe(true);
    // Plain positional notation on re-encode (matches `rust_decimal`'s
    // `Display` — see `models.ts.j2`'s `Decimal` export doc comment).
    expect((decoded.amountXaf as InstanceType<typeof Decimal>).toString()).toBe("0.0000001");
  });

  it("preserves precision beyond rust_decimal's ~28-29 significant-digit capacity", () => {
    const wireValue = "1.234567890123456789012345678901234567890E+10";
    const value = { id: 2, amountXaf: new Decimal(wireValue) };
    const bytes = encodeMessage(value, INVOICE_FIELDS, {}, {});
    const decoded = decodeMessage(bytes, INVOICE_FIELDS, {}, {});

    expect((decoded.amountXaf as InstanceType<typeof Decimal>).toString()).toBe(
      "12345678901.2345678901234567890123456789",
    );
  });
});
"#;

#[test]
fn grpc_decimal_round_trips_through_encode_decode_message() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping grpc_decimal_round_trips_through_encode_decode_message: \
             `node`/`npm`/`npx` not on PATH (expected in this repo's Rust-only CI jobs — \
             see this test's module doc)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema(SCHEMA).expect("inline schema should parse");
    let extra = cratestack_proto::synthesize_messages(&schema).expect("should synthesize");
    let mut lock =
        cratestack_proto::build_lock(&schema, None, &extra).expect("should build a fresh lock");
    lock.package = Some("grpc_decimal_pkg".to_owned());

    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "@example/grpc-decimal".to_owned(),
            base_path: "/".to_owned(),
            template_dir: None,
            preset: TypeScriptPreset::Default,
            full_selection: false,
            pb_lock: Some(lock),
            schema_sha256: "unused-for-grpc-web".to_owned(),
        },
    )
    .expect("default template should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    let js_fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/decimal_round_trip");
    for asset in ["package.json", "vitest.config.ts"] {
        fs::copy(format!("{js_fixture_dir}/{asset}"), dir.path().join(asset))
            .unwrap_or_else(|error| panic!("copy {asset} into generated package dir: {error}"));
    }
    fs::write(dir.path().join("decimal.test.ts"), CHECK_TEST).expect("write check test");

    let install = Command::new("npm")
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

    let test_run = Command::new("npx")
        .args(["--yes", "vitest", "run"])
        .current_dir(dir.path())
        .output()
        .expect("run npx vitest");

    let stdout = String::from_utf8_lossy(&test_run.stdout);
    let stderr = String::from_utf8_lossy(&test_run.stderr);
    assert!(
        test_run.status.success(),
        "vitest run against the generated gRPC-Web Decimal wire kind failed — this is the \
         real round-trip proof for cratestack#499 F4, not a Rust string assertion:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("2 passed") || stderr.contains("2 passed"),
        "expected vitest to report exactly 2 passed tests:\nstdout: {stdout}\nstderr: {stderr}"
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
