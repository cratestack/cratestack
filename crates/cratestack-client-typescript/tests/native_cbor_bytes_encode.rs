//! Real, Node-driven proof that a `Bytes` field reaches the wire in the
//! shape a server-side `Vec<u8>` accepts — a CBOR **byte string** (RFC
//! 8949 major type 2) under the native codec, and a JSON integer array
//! under `jsonRpcCodec`.
//!
//! ## Two independent defects, one symptom
//!
//! This file covers cratestack#806 **and** cratestack#820, because
//! neither is observable without the other being fixed:
//!
//! - **#806** — no *published* `@cratestack/cbor` below `0.8.15` encoded
//!   a `Uint8Array` as a byte string. Fixed by raising
//!   `CRATESTACK_CBOR_FLOOR`.
//! - **#820** — the generated client's own `encodeWireFields` rebuilt
//!   every `Uint8Array` into `{"0":1,"1":2,"2":3}` *before* either codec
//!   ran, so the codec never saw a typed array to encode correctly.
//!
//! Raising the floor alone changed nothing observable; fixing
//! `encodeWireFields` alone would still have shipped map-encoded bytes on
//! any consumer resolving `^0.8.0`. That is why the first version of this
//! test failed at the raised floor, and why it is worth stating here: a
//! green result depends on both, and a future regression in either one
//! turns it red.
//!
//! ## Why this asserts raw bytes rather than a decoded value
//!
//! This is the trap #806 calls out, and it is worth being explicit about
//! because the obvious test does not work. The defect is invisible at the
//! type level — `Uint8Array` typechecks identically against the broken and
//! fixed codecs — and it is *also* invisible to a decode-side round trip,
//! because the same broken codec that encodes a `Uint8Array` as a CBOR map
//! will happily decode that map back into something that looks right to a
//! JS `deepEqual`. What it cannot do is produce the bytes a server-side
//! `Vec<u8>` accepts.
//!
//! So the assertion here is on the first byte of the encoded field:
//!
//! ```text
//! 0x43 010203   major type 2, length 3  -> byte string   (correct)
//! 0xa3 61300161…  major type 5, 3 pairs -> map           (the #806 defect)
//! ```
//!
//! A test that decoded and compared would have passed against
//! `@cratestack/cbor@0.8.14` — the exact version that ships the bug.
//!
//! ## Why the real published package, not a stub
//!
//! Same reason `native_cbor_decimal_encode.rs` gives: this crate's stub
//! codec is `JSON.stringify` underneath, and a stub-based test is
//! precisely the shape of test that let the defect ship. `npm install`
//! here resolves `@cratestack/cbor` through the generated `package.json`,
//! whose constraint comes from `CRATESTACK_CBOR_FLOOR` — so this test is
//! also, indirectly, an assertion that the floor names a version that
//! actually fixes `Bytes`.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm` aren't on
//! `PATH` — same convention as every other Node-driven test here.

use std::io::Write as _;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const FIXTURE: &str = "tests/fixtures/bytes_native_cbor_encode.cstack";

#[test]
fn a_bytes_field_reaches_the_wire_as_a_cbor_byte_string_under_the_floor_codec() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping a_bytes_field_reaches_the_wire_as_a_cbor_byte_string_under_the_floor_codec: \
             `node`/`npm` not on PATH (expected only where Node is absent, e.g. a local Rust-only \
             checkout; CI runs this)"
        );
        return;
    }

    let dir = generate_install_and_write_package();

    let script_path = dir.path().join("smoke.mts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ CratestackRpcRuntime }} from "./src/runtime.js";
import {{ AttachmentApi }} from "./src/client.js";
import {{ createCborCodec }} from "@cratestack/cbor";

let capturedBody: Uint8Array | undefined;

const stubFetch: typeof fetch = async (_url, init) => {{
  const body = init?.body;
  if (!(body instanceof Uint8Array)) {{
    throw new Error(`expected the native codec to hand fetch() a Uint8Array body, got ${{typeof body}}`);
  }}
  capturedBody = body;
  const codec = await createCborCodec();
  const responseBytes = codec.encode({{
    id: "att_1",
    label: "receipt",
    payload: new Uint8Array([1, 2, 3]),
    checksums: [1, 2, 3],
  }});
  return new Response(new Uint8Array(responseBytes), {{
    status: 200,
    headers: {{ "Content-Type": "application/cbor" }},
  }});
}};

const runtime = new CratestackRpcRuntime("http://example.invalid", {{ fetch: stubFetch }});
const api = new AttachmentApi(runtime);

const created = await api.create({{
  label: "receipt",
  payload: new Uint8Array([1, 2, 3]),
  checksums: [1, 2, 3],
}});

if (capturedBody === undefined) {{
  throw new Error("fetch was never called — codec.encode() must have thrown beforehand");
}}

const hex = Buffer.from(capturedBody).toString("hex");

// THE ASSERTION THAT MATTERS. `43 010203` is CBOR major type 2 (byte
// string) of length 3. The #806 defect emitted `a3 613001 613102 613203`
// — major type 5, a map — which decodes back to something plausible in JS
// and is undecodable as a server-side `Vec<u8>`.
if (!hex.includes("43010203")) {{
  throw new Error(
    "the Bytes field did not reach the wire as a CBOR byte string. Expected the encoded body to " +
    "contain `43010203` (major type 2, length 3). This is cratestack#806: a codec below " +
    "@cratestack/cbor@0.8.15 walks a Uint8Array as a plain object and emits a CBOR map " +
    `(a3 613001 …) that no server-side Vec<u8> can decode.\nfull body hex: ${{hex}}`,
  );
}}

// The negative half, and the reason the fixture carries an Int[] too:
// prove the codec is DISCRIMINATING rather than coincidentally right. An
// `Int[]` must stay an array (major type 4, `83 010203`) — if both fields
// encoded identically, `43010203` above would prove nothing about Bytes
// specifically.
if (!hex.includes("83010203")) {{
  throw new Error(
    "the Int[] field did not reach the wire as a CBOR array (`83010203`, major type 4). Bytes " +
    "and Int[] must encode differently; if they do not, the byte-string assertion above is not " +
    `evidence about Bytes.\nfull body hex: ${{hex}}`,
  );
}}

// Decode-side sanity: the response's Bytes must come back as a real
// Uint8Array, not an index-keyed object.
if (!(created.payload instanceof Uint8Array)) {{
  throw new Error(
    `expected the create() response payload to decode into a Uint8Array, got ` +
    `${{Object.prototype.toString.call(created.payload)}}`,
  );
}}

console.log("NATIVE_CBOR_BYTES_ENCODE_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.mts");
    let output = tsx.output().expect("run tsx");

    assert!(
        output.status.success(),
        "a Bytes field did not reach the wire as a CBOR byte string under the published \
         @cratestack/cbor resolved at CRATESTACK_CBOR_FLOOR:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("NATIVE_CBOR_BYTES_ENCODE_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );
}

/// The other half of cratestack#820: the same `encodeWireFields` walk
/// runs on the JSON RPC path too, so `--no-native-cbor` was broken by the
/// identical cause in a different disguise.
///
/// `encodeBinaryAsJson` exists precisely to turn a `Uint8Array` into the
/// integer array a server-side `Vec<u8>` accepts — but it ran *after*
/// `encodeWireFields` had already rebuilt the typed array into
/// `{"0":1,"1":2,"2":3}`, so its `Array.from` never fired. Asserted
/// directly on the two functions rather than through a request, because
/// their ordering is the whole defect.
#[test]
fn the_json_rpc_path_encodes_bytes_as_an_integer_array_not_an_index_keyed_object() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping the_json_rpc_path_encodes_bytes_as_an_integer_array_not_an_index_keyed_object: \
             `node`/`npm` not on PATH (expected only where Node is absent; CI runs this)"
        );
        return;
    }

    // `models.ts` imports `decimal.js`, so this needs a real install even
    // though the assertion never touches the native codec.
    let dir = generate_install_and_write_package();

    let script_path = dir.path().join("smoke.mts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ encodeWireFields, encodeBinaryAsJson }} from "./src/models.js";

// The exact composition `jsonRpcCodec` sees: terminalLink applies
// encodeWireFields, then the codec applies encodeBinaryAsJson.
const wire = JSON.stringify(encodeBinaryAsJson(encodeWireFields({{
  payload: new Uint8Array([1, 2, 3]),
  checksums: [1, 2, 3],
}})));

if (wire !== '{{"payload":[1,2,3],"checksums":[1,2,3]}}') {{
  throw new Error(
    "the JSON RPC path did not encode Bytes as an integer array. Expected " +
    '{{"payload":[1,2,3],"checksums":[1,2,3]}} — cratestack#820: encodeWireFields rebuilt the ' +
    "Uint8Array into an index-keyed object before encodeBinaryAsJson could convert it, so " +
    `Array.from never fired.\ngot: ${{wire}}`,
  );
}}

console.log("JSON_RPC_BYTES_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.mts");
    let output = tsx.output().expect("run tsx");
    assert!(
        output.status.success(),
        "the JSON RPC encode path mangled a Bytes field:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("JSON_RPC_BYTES_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );
}

/// Generate the package, write it out, and `npm install` it.
///
/// The install is not incidental: it resolves the REAL `@cratestack/cbor`
/// through the generated `package.json`, whose constraint comes from
/// `CRATESTACK_CBOR_FLOOR`. A failure here usually means the floor names a
/// version the registry cannot serve — cratestack#754/#779's defect class.
fn generate_install_and_write_package() -> tempfile::TempDir {
    let dir = generate_and_write_package();
    let mut install = Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed (this resolves the real @cratestack/cbor at CRATESTACK_CBOR_FLOOR):\n{}",
        command_report(&install, &installed)
    );
    dir
}

fn generate_and_write_package() -> tempfile::TempDir {
    let schema = cratestack_parser::parse_schema_file(FIXTURE)
        .unwrap_or_else(|error| panic!("fixture {FIXTURE:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "bytes-native-cbor-encode-check".to_owned(),
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("default template should render: {error}"));

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }
    dir
}
