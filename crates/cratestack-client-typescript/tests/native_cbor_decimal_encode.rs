//! Real, Node-driven proof for the P1 regression this file exists to
//! guard: `@cratestack/cbor` (the default RPC codec since #746,
//! `native_cbor_generator.rs`) throws encoding ANY request body that
//! carries a real `decimal.js` `Decimal` instance — `create`/`update`
//! inputs, procedure arguments, and `batch()` frames all reach the exact
//! same `codec.encode()` call site.
//!
//! ## Why the actually-published `@cratestack/cbor`, not a stub
//!
//! `native_cbor_generator.rs`'s own
//! `native_codec_factory_is_memoized_and_retried_after_a_rejection` test
//! substitutes a hand-written stub `@cratestack/cbor` module — deliberately,
//! for a different property (memoization/retry, not encode correctness).
//! That stub's `encode()` is `JSON.stringify` underneath, which happily
//! serializes a `Decimal` via `toJSON()` — so a stub-based test not only
//! wouldn't catch this bug, it is *precisely* the shape of test that let
//! it ship in #752 in the first place: nothing in this crate's suite ran
//! the real native addon against a real `Decimal` instance. This file
//! `npm install`s the real `@cratestack/cbor` from the registry and
//! decodes the captured wire bytes with it too — the round trip that
//! matters is entirely inside the real compiled Rust codec, not a JS
//! reimplementation of it.
//!
//! ## Why a new file rather than extending an existing one
//!
//! `decimal_round_trip.rs`/`decimal_relation_and_procedure_round_trip.rs`
//! (cratestack#498/#499) are REST-transport-only and assert through
//! `vitest` against `JSON.stringify`-based bodies — neither exercises the
//! RPC codec seam at all, which is the actual gap that let this bug
//! escape (noted in this ticket's own review). `native_cbor_generator.rs`
//! is close in *subject* (the `native_cbor` flag) but wrong in
//! *mechanism* for this proof: every test in it but one is a source-text
//! assertion, and the one real Node-driven test in it deliberately stubs
//! the codec for an unrelated property (see above). This file's mechanism
//! — real npm-installed `@cratestack/cbor`, custom `fetch` capturing the
//! raw encoded bytes, decoding them back with the same real codec — is
//! novel enough (and specific enough to the encode-side P1 fix) to
//! deserve its own file rather than being wedged into either.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm` aren't on
//! `PATH` — same convention as every other Node-driven test in this
//! crate.

use std::io::Write as _;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const FIXTURE: &str = "tests/fixtures/decimal_native_cbor_encode.cstack";

/// The primary proof: `InvoiceApi.create()` — a `Decimal`-carrying
/// `create` input, the exact call shape #752's own bug report hit — must
/// reach the wire as a plain string under the real published
/// `@cratestack/cbor`, not throw before the request is ever sent.
#[test]
fn create_input_carrying_a_real_decimal_instance_encodes_to_a_plain_string_under_the_real_native_codec()
 {
    if !node_toolchain_available() {
        eprintln!(
            "skipping create_input_carrying_a_real_decimal_instance_encodes_to_a_plain_string_under_the_real_native_codec: \
             `node`/`npm` not on PATH (expected in this repo's Rust-only CI jobs)"
        );
        return;
    }

    let dir = generate_and_write_package();

    let mut install = Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed (this installs the REAL @cratestack/cbor from the registry, not a \
         stub):\n{}",
        command_report(&install, &installed)
    );

    let script_path = dir.path().join("smoke.mts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ CratestackRpcRuntime }} from "./src/runtime.js";
import {{ InvoiceApi }} from "./src/client.js";
import {{ Decimal }} from "./src/models.js";
import {{ createCborCodec }} from "@cratestack/cbor";

let capturedBody: Uint8Array | undefined;

const stubFetch: typeof fetch = async (_url, init) => {{
  const body = init?.body;
  if (!(body instanceof Uint8Array)) {{
    throw new Error(`expected the native codec to hand fetch() a Uint8Array body, got ${{typeof body}}`);
  }}
  capturedBody = body;
  const codec = await createCborCodec();
  const responseBytes = codec.encode({{ id: "inv_1", reference: "INV-1", amountXaf: "1.5" }});
  return new Response(new Uint8Array(responseBytes), {{
    status: 200,
    headers: {{ "Content-Type": "application/cbor" }},
  }});
}};

const runtime = new CratestackRpcRuntime("http://example.invalid", {{ fetch: stubFetch }});
const api = new InvoiceApi(runtime);

let created;
try {{
  created = await api.create({{ reference: "INV-1", amountXaf: new Decimal("1.5") }});
}} catch (error) {{
  throw new Error(
    "api.create() threw encoding a Decimal-carrying request body — this is the exact P1 " +
    `regression (@cratestack/cbor can't serialize a real Decimal instance via its own ` +
    `enumerable properties): ${{(error as Error).message}}`,
  );
}}

if (capturedBody === undefined) {{
  throw new Error("fetch was never called — codec.encode() must have thrown beforehand");
}}

// Decode the ACTUAL wire bytes with the same real, published codec —
// proving what reached the network, not what the client claims to have
// sent.
const codec = await createCborCodec();
const decodedRequest = codec.decode(capturedBody) as Record<string, unknown>;

if (typeof decodedRequest.amountXaf !== "string") {{
  throw new Error(
    `expected amountXaf to reach the wire as a plain string, got typeof ` +
    `${{typeof decodedRequest.amountXaf}} (${{JSON.stringify(decodedRequest.amountXaf)}}) — a ` +
    `pre-fix build would have thrown before this point, so seeing a non-string here at all ` +
    `means encodeWireFields regressed differently`,
  );
}}
if (decodedRequest.amountXaf !== "1.5") {{
  throw new Error(`expected amountXaf to decode to "1.5" on the wire, got: ${{decodedRequest.amountXaf}}`);
}}

if (!(created.amountXaf instanceof Decimal) || !created.amountXaf.equals(new Decimal("1.5"))) {{
  throw new Error("the create() response's amountXaf did not decode back into a real Decimal");
}}

console.log("NATIVE_CBOR_DECIMAL_ENCODE_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.mts");
    let output = tsx.output().expect("run tsx");

    assert!(
        output.status.success(),
        "generated RPC client failed to encode a Decimal-carrying create() input under the \
         real, published @cratestack/cbor:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("NATIVE_CBOR_DECIMAL_ENCODE_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );
}

/// Coverage bullet: a `batch()` payload is an array of `RpcRequest`
/// frames, each with its own `input` — a `create` frame and a `quote`
/// procedure frame (a *procedure argument*, not a model field, cratestack#746
/// follow-up's other reach point) in the same batch, both carrying a real
/// `Decimal`. Proves `terminalLink`'s single `encodeWireFields(request.input)`
/// call recurses through the whole frame array, not just a top-level
/// object.
#[test]
fn batch_payload_with_two_decimal_carrying_frames_encodes_both_under_the_real_native_codec() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping batch_payload_with_two_decimal_carrying_frames_encodes_both_under_the_real_native_codec: \
             `node`/`npm` not on PATH (expected in this repo's Rust-only CI jobs)"
        );
        return;
    }

    let dir = generate_and_write_package();

    let mut install = Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed:\n{}",
        command_report(&install, &installed)
    );

    let script_path = dir.path().join("smoke.mts");
    let mut script = std::fs::File::create(&script_path).expect("create smoke script");
    write!(
        script,
        r#"
import {{ CratestackRpcRuntime }} from "./src/runtime.js";
import {{ Decimal }} from "./src/models.js";
import {{ createCborCodec }} from "@cratestack/cbor";

let capturedBody: Uint8Array | undefined;

const stubFetch: typeof fetch = async (_url, init) => {{
  const body = init?.body;
  if (!(body instanceof Uint8Array)) {{
    throw new Error(`expected a Uint8Array batch body, got ${{typeof body}}`);
  }}
  capturedBody = body;
  const codec = await createCborCodec();
  const responseBytes = codec.encode([
    {{ id: 1, output: {{ id: "inv_2", reference: "INV-2", amountXaf: "3.25" }} }},
    {{ id: 2, output: "9.75" }},
  ]);
  return new Response(new Uint8Array(responseBytes), {{
    status: 200,
    headers: {{ "Content-Type": "application/cbor" }},
  }});
}};

const runtime = new CratestackRpcRuntime("http://example.invalid", {{ fetch: stubFetch }});

let frames;
try {{
  frames = await runtime.batch([
    {{ id: 1, op: "model.Invoice.create", input: {{ reference: "INV-2", amountXaf: new Decimal("3.25") }} }},
    {{ id: 2, op: "procedure.quote", input: {{ amount: new Decimal("9.75") }} }},
  ]);
}} catch (error) {{
  throw new Error(`runtime.batch() threw encoding Decimal-carrying frames: ${{(error as Error).message}}`);
}}
if (frames.length !== 2) {{
  throw new Error(`expected 2 response frames, got ${{frames.length}}`);
}}

if (capturedBody === undefined) {{
  throw new Error("fetch was never called");
}}

const codec = await createCborCodec();
const decodedRequest = codec.decode(capturedBody) as Array<{{ id: number; op: string; input: unknown }}>;
if (decodedRequest.length !== 2) {{
  throw new Error(`expected 2 request frames on the wire, got ${{decodedRequest.length}}`);
}}

const createFrame = decodedRequest.find((frame) => frame.id === 1)!;
const createInput = createFrame.input as Record<string, unknown>;
if (typeof createInput.amountXaf !== "string" || createInput.amountXaf !== "3.25") {{
  throw new Error(
    `expected the batch's create frame amountXaf to be the plain string "3.25" on the wire, got: ` +
    `${{JSON.stringify(createInput.amountXaf)}}`,
  );
}}

const quoteFrame = decodedRequest.find((frame) => frame.id === 2)!;
const quoteInput = quoteFrame.input as Record<string, unknown>;
if (typeof quoteInput.amount !== "string" || quoteInput.amount !== "9.75") {{
  throw new Error(
    `expected the batch's quote frame amount (a procedure argument) to be the plain string ` +
    `"9.75" on the wire, got: ${{JSON.stringify(quoteInput.amount)}}`,
  );
}}

console.log("NATIVE_CBOR_BATCH_DECIMAL_ENCODE_OK");
"#
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(dir.path(), "smoke.mts");
    let output = tsx.output().expect("run tsx");

    assert!(
        output.status.success(),
        "generated RPC client failed to encode Decimal-carrying batch() frames under the real, \
         published @cratestack/cbor:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("NATIVE_CBOR_BATCH_DECIMAL_ENCODE_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );
}

fn generate_and_write_package() -> tempfile::TempDir {
    let schema = cratestack_parser::parse_schema_file(FIXTURE)
        .unwrap_or_else(|error| panic!("fixture {FIXTURE:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "decimal-native-cbor-encode-check".to_owned(),
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
