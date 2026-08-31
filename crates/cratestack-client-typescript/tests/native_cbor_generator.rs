//! `native_cbor` (issue #746): gates whether the generated RPC runtime's
//! default codec is `@cratestack/cbor`'s `createCborCodec()` instead of the
//! pure-TypeScript `jsonRpcCodec`. Mirrors the shape of
//! `cratestack-client-dart`'s `tests/native_cbor_generator.rs` (issue #563's
//! own regression test): a genuine reads-the-real-default guard, a
//! presence/shape check with the flag off (CLI: `--no-native-cbor`), and an
//! over-emission guard proving the flag only touches the files that
//! legitimately depend on the codec choice.
//!
//! **Native is now the default** (`TypeScriptGeneratorConfig::
//! DEFAULT_NATIVE_CBOR` is `true` — see `native_cbor`'s field doc comment
//! for the full history and the one open platform gap: `@cratestack/cbor-node`
//! ships no musl/`win32-arm64` build). `default_config_uses_native_cbor`
//! below reads `TypeScriptGeneratorConfig::default()` directly (never a
//! hardcoded bool) so it fails if the constant is ever flipped back without
//! updating this test — the same anti-pattern Dart's own doc comment for
//! this test file calls out and that `tests/snapshot.rs`'s pre-existing
//! (pre-#746) helper hardcodes for other flags; deliberately not repeated
//! here.
//!
//! **REST is out of scope for this ticket** — `rest-runtime.ts.j2` has no
//! codec seam at all, so every test below also asserts the flag has zero
//! effect on a REST-transport schema's output.
//!
//! Mostly structural coverage (source-level assertions, no real `npm
//! install`/`tsc`/round trip against the *published* `@cratestack/cbor` —
//! see this crate's `tests/swr_paged_model_tsc.rs`/`tests/node_dist_esm.rs`
//! for the established "real compiler against the published package"
//! pattern this ticket's own Test Plan calls for as a follow-up CI job,
//! not a `cargo test`), with ONE exception:
//! `native_codec_factory_is_memoized_and_retried_after_a_rejection` below
//! actually runs the generated runtime under Node against a stub
//! `@cratestack/cbor` module, because the memoization/retry contract
//! (issue #746's review finding #1: a rejected codec-resolution promise
//! must not be memoized forever) is a genuine behavioral property that no
//! amount of source-text matching can prove — see that test's own doc
//! comment for the deliberate-break verification that confirms it
//! actually fails when the fix is reverted.

use std::process::Command;

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const REST_FIXTURE: &str = "tiny_rest";
const RPC_FIXTURE: &str = "tiny_rpc";

/// cratestack#779: the `@cratestack/cbor` API floor a generated
/// `package.json` declares, restated here as a **literal** rather than
/// recomputed from `env!("CARGO_PKG_VERSION")`.
///
/// This file used to carry a private `minor_floor_version_requirement()`
/// that mirrored the generator's own `^{major}.{minor}.0` computation,
/// justified as "asserts against the same computation the generator
/// actually performs rather than a second hardcoded literal that could
/// silently drift from it". That reasoning is backwards for this
/// property: agreeing with the generator by construction is exactly what
/// made the assertion unable to observe #779. Both sides moved together
/// on a bump and the test stayed green while the emitted range walked off
/// the published registry.
///
/// A literal is the tripwire. At the next version this still expects the
/// value below; a generator that has gone back to deriving emits
/// `^0.9.0` and fails here — on the bump PR, which is where #779's
/// damage lands.
///
/// Kept in sync by hand with
/// `cratestack_client_typescript::package_floors::CRATESTACK_CBOR_FLOOR`
/// (`pub(crate)`, so not callable from an integration test).
///
/// **Moved `^0.8.0` -> `^0.8.15` for cratestack#806**, and the way that
/// happened is the tripwire working rather than an inconvenience: raising
/// the real constant turned this test red, forcing the second edit to be
/// a deliberate act with a reason attached. A derived value would have
/// followed silently — which is the whole failure #779 removed. If you
/// are here because this assertion failed, do not "fix" it by deriving;
/// check that the new floor names a version npm actually serves, then
/// update this literal too.
const CRATESTACK_CBOR_FLOOR: &str = "0.8.15";

/// Reads `CRATESTACK_CBOR_FLOOR` out of `src/package_floors.rs`.
///
/// Used ONLY by [`literal_matches_the_real_floor`], never to build an
/// expectation — deriving the expectation is precisely what the constant
/// above forbids. A line scan rather than a Rust parse, matching
/// `package_floors_tests.rs`'s own pubspec reader: one constant, known
/// shape, and pulling in a parser to read it would be the larger risk.
fn real_cbor_floor() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/package_floors.rs");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("reading {path} for CRATESTACK_CBOR_FLOOR: {error}"));
    source
        .lines()
        .find_map(|line| {
            let rest = line
                .trim()
                .strip_prefix("pub(crate) const CRATESTACK_CBOR_FLOOR: &str = \"")?;
            rest.strip_suffix("\";").map(str::to_owned)
        })
        .unwrap_or_else(|| {
            panic!(
                "could not find `pub(crate) const CRATESTACK_CBOR_FLOOR: &str = \"...\";` in \
                 {path} — was it renamed or reformatted? This drift check is meaningless until \
                 it can read the real floor."
            )
        })
}

/// States the tripwire as its own assertion, so a disagreement reads as one
/// clear line rather than as unrelated "package.json must depend on
/// @cratestack/cbor" failures dumping whole generated files.
///
/// This does NOT weaken the literal above — the expectation stays hand-
/// written, and raising the real floor still turns this red and still
/// demands a deliberate second edit. It only makes the reason legible. The
/// Dart crate carries the identical pair; adding it there was prompted by
/// that failure mode being misread as noise and "fixed" by deriving (#845),
/// which was reverted.
/// The full requirement the generator emits for this floor: the
/// hand-written literal above, plus the ceiling derived from the release
/// line.
///
/// Composing the ceiling here rather than hardcoding it is deliberate and
/// does NOT weaken the tripwire. The two halves have opposite rules — the
/// floor is a hand-verified fact about published archives, the ceiling is
/// mechanical and by design names a version that does not exist yet.
/// Hardcoding the ceiling would turn this test red at every minor bump for
/// no defect, which is exactly the noise that got the tripwire misread and
/// deleted once already (#845, reverted by #849). The arithmetic it relies
/// on is pinned separately against a hand-written table in
/// `src/release_line_tests.rs`, so it is not agreeing with itself.
fn expected_requirement() -> String {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let mut component = |which: &str| -> u64 {
        parts
            .next()
            .unwrap_or_else(|| panic!("CARGO_PKG_VERSION has no {which} component"))
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|error| panic!("CARGO_PKG_VERSION's {which} component: {error}"))
    };
    let major = component("major");
    let minor = component("minor");
    format!(">={CRATESTACK_CBOR_FLOOR} <{major}.{}.0", minor + 1)
}

#[test]
fn literal_matches_the_real_floor() {
    let real = real_cbor_floor();
    assert_eq!(
        CRATESTACK_CBOR_FLOOR, real,
        "this file's CRATESTACK_CBOR_FLOOR literal ({CRATESTACK_CBOR_FLOOR}) disagrees with \
         src/package_floors.rs ({real}).\n\nThis is the tripwire, not a bug: raising the real \
         floor is meant to force a deliberate second edit here. Confirm {real} names a version \
         npm actually serves, then update the literal in this file to match. Do NOT derive it — \
         see the constant's doc comment for why."
    );
}

#[test]
fn default_config_uses_native_cbor() {
    let config = TypeScriptGeneratorConfig::default();
    assert!(
        config.native_cbor,
        "TypeScriptGeneratorConfig::default().native_cbor must be true (DEFAULT_NATIVE_CBOR) \
         now that issue #746 makes @cratestack/cbor the default RPC codec"
    );

    let rpc = generate(RPC_FIXTURE, config.clone());
    let package_json = file(&rpc, "package.json");
    assert!(
        package_json.contains(&format!(
            "\"@cratestack/cbor\": \"{}\"",
            expected_requirement()
        )),
        "RPC: default config's package.json must depend on @cratestack/cbor, pinned to this \
         crate's API floor (a constant, not anything derived from CARGO_PKG_VERSION — see \
         cratestack_client_typescript::package_floors' module doc, and cratestack#707/#779 \
         for why):\n{package_json}"
    );
    let runtime = file(&rpc, "src/runtime.ts");
    assert!(
        runtime.contains("import { createCborCodec } from \"@cratestack/cbor\";"),
        "RPC: default config's runtime.ts must import createCborCodec:\n{runtime}"
    );
    assert!(
        !runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "RPC: default config's runtime.ts must not default to jsonRpcCodec:\n{runtime}"
    );

    let rest = generate(REST_FIXTURE, config);
    let rest_package_json = file(&rest, "package.json");
    assert!(
        !rest_package_json.contains("@cratestack/cbor"),
        "REST: native_cbor must have no effect — package.json must never mention \
         @cratestack/cbor:\n{rest_package_json}"
    );
}

#[test]
fn no_native_cbor_falls_back_to_json_rpc_codec() {
    let plain = generate(
        RPC_FIXTURE,
        TypeScriptGeneratorConfig {
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
    );

    let package_json = file(&plain, "package.json");
    assert!(
        !package_json.contains("@cratestack/cbor"),
        "native_cbor: false package.json must not mention @cratestack/cbor:\n{package_json}"
    );

    let runtime = file(&plain, "src/runtime.ts");
    assert!(
        runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "native_cbor: false runtime.ts must default to jsonRpcCodec:\n{runtime}"
    );
    assert!(
        !runtime.contains("@cratestack/cbor"),
        "native_cbor: false runtime.ts must not mention @cratestack/cbor:\n{runtime}"
    );
    assert!(
        !runtime.contains("createCborCodec"),
        "native_cbor: false runtime.ts must not reference createCborCodec:\n{runtime}"
    );
}

#[test]
fn the_flag_swaps_the_package_json_dependency_and_the_runtime_codec_resolution() {
    let native = generate(RPC_FIXTURE, TypeScriptGeneratorConfig::default());

    let package_json = file(&native, "package.json");
    assert!(
        package_json.contains(&format!(
            "\"@cratestack/cbor\": \"{}\"",
            expected_requirement()
        )),
        "package.json should depend on @cratestack/cbor, pinned to the API floor constant \
         (cratestack#779 — not derived from CARGO_PKG_VERSION at any precision, including the \
         `^{{major}}.{{minor}}.0` shape that used to be emitted here):\n{package_json}"
    );

    let runtime = file(&native, "src/runtime.ts");
    assert!(
        runtime.contains("import { createCborCodec } from \"@cratestack/cbor\";"),
        "runtime.ts should import createCborCodec:\n{runtime}"
    );
    assert!(
        !runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "runtime.ts must not also default to jsonRpcCodec under the flag:\n{runtime}"
    );
    assert!(
        runtime.contains("private resolveCodec(): Promise<CratestackRpcCodec> {"),
        "runtime.ts should define the memoized async codec resolver:\n{runtime}"
    );
    assert!(
        runtime.contains("private readonly explicitCodec: CratestackRpcCodec | undefined;"),
        "runtime.ts should capture an explicitly-supplied options.codec separately from the \
         lazily-resolved native codec:\n{runtime}"
    );
    assert!(
        runtime.contains("private codecPromise: Promise<CratestackRpcCodec> | undefined;"),
        "runtime.ts should memoize the resolved codec in a cached Promise field:\n{runtime}"
    );
}

/// The Technical Context's own call-site inventory: `call()`, `batch()`,
/// `stream()` and `readUnaryResponse()` — 4 already-`async` methods — must
/// each `await this.resolveCodec()` exactly once, and `createCborCodec()`
/// itself must be invoked from exactly one place (the memoized resolver),
/// never inline at a call site (that would defeat the "at most once per
/// runtime instance" memoization the doc comment promises).
///
/// This is a structural check only — it constrains *shape* (one call
/// expression, not N), not *behavior*. It deliberately does NOT hardcode
/// the exact operator the resolver uses internally (a prior version of
/// this test matched the literal string `"this.codecPromise ??=
/// createCborCodec());"`, which is that one review finding: a plain `??=`
/// never retries after a *rejected* promise, so that string match would
/// have kept passing even with the retry-on-rejection bug still present,
/// since it only asserted the buggy line existed, not that retry actually
/// works). The real behavioral proof — that the factory really is called
/// once across N calls, and really is retried after a rejection instead
/// of replaying it forever — is
/// `native_codec_factory_is_memoized_and_retried_after_a_rejection`
/// below, which actually runs the generated runtime under Node.
#[test]
fn native_codec_call_sites_are_structurally_sound() {
    let native = generate(RPC_FIXTURE, TypeScriptGeneratorConfig::default());
    let runtime = file(&native, "src/runtime.ts");

    assert_eq!(
        runtime
            .matches("const codec = await this.resolveCodec();")
            .count(),
        4,
        "call()/batch()/stream()/readUnaryResponse() should each resolve the codec exactly \
         once:\n{runtime}"
    );
    // Doc comments legitimately mention `createCborCodec()` by name more
    // than once; the actual invocation (as opposed to prose referencing
    // it) is the one `createCborCodec().catch(...)` expression inside
    // `resolveCodec()`.
    assert_eq!(
        runtime.matches("createCborCodec().catch(").count(),
        1,
        "createCborCodec() must be invoked from exactly one place (the memoized resolveCodec() \
         accessor), never per-request:\n{runtime}"
    );
    assert!(
        !runtime.contains("readonly codec: CratestackRpcCodec;"),
        "the old synchronous public `codec` field must not survive alongside the native \
         codec path:\n{runtime}"
    );
    assert!(
        !runtime.contains("this.codec.") && !runtime.contains("this.codec ="),
        "no call site on the native path should still read/write a plain `this.codec` field \
         (it was replaced by `explicitCodec`/`codecPromise` plus the local `codec` each method \
         resolves):\n{runtime}"
    );
}

/// Genuine behavioral proof for issue #746's finding #1/#5: actually runs
/// the generated `CratestackRpcRuntime` under Node (via `tsx`, no
/// build step needed — this module's own imports are all relative +
/// `@cratestack/cbor`, so unlike `tests/swr_runtime.rs`/`node_dist_esm.rs`
/// no `npm install` is required first) against a stub `@cratestack/cbor`
/// module substituted into a real `node_modules/@cratestack/cbor`
/// directory, and asserts on OBSERVED behavior rather than source text:
///
///   1. Three `runtime.call()`s against a fresh runtime invoke the stub
///      factory exactly once — real memoization, not a string match on
///      the memoizing expression.
///   2. A second, freshly-constructed runtime whose first codec
///      resolution is made to reject still succeeds on its NEXT call,
///      and the factory is observed being invoked again (retried) rather
///      than the same rejection being replayed — this is finding #1's
///      exact regression: a `??=`-based memo would leave every
///      subsequent call throwing the same stale rejection forever, which
///      this test would catch by the process exiting non-zero on the
///      second `runtime2.call()`.
///
/// Confirmed to actually fail (not just theoretically): reverting
/// `resolveCodec()` to the old `return (this.codecPromise ??=
/// createCborCodec());` form and running this exact stub script makes
/// the second `runtime2.call()` throw the stub's rejection a second
/// time, exiting non-zero — verified by hand while writing this test,
/// not asserted from reading the diff alone.
///
/// Degrades to a printed skip when `node`/`npm` aren't on `PATH`, same
/// convention as `tests/swr_runtime.rs`/`tests/node_dist_esm.rs` — a local
/// Rust-only checkout; CI's `ubuntu-latest` ships Node, so this runs there.
#[test]
fn native_codec_factory_is_memoized_and_retried_after_a_rejection() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping native_codec_factory_is_memoized_and_retried_after_a_rejection: \
             `node`/`npm` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let native = generate(RPC_FIXTURE, TypeScriptGeneratorConfig::default());

    let root = tempfile::tempdir().expect("tempdir");
    let pkg_dir = root.path().join("pkg");
    for file in &native.files {
        let path = pkg_dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    // `runtime.ts` imports `encodeWireFields` from `./models.js` (the
    // P1 fix for `@cratestack/cbor` throwing on a real `Decimal`
    // instance), which in turn imports `decimal.js` — so this test's
    // hand-built `node_modules` (below) needs a real `decimal.js`
    // alongside the stub `@cratestack/cbor`, installed into the same
    // `root` ancestor directory so Node's resolution walk-up finds it
    // from `pkg/src/models.ts`.
    let install_decimal = Command::new("npm")
        .args([
            "install",
            "--no-save",
            "--no-audit",
            "--no-fund",
            "decimal.js",
        ])
        .current_dir(root.path())
        .output()
        .expect("run npm install (decimal.js)");
    assert!(
        install_decimal.status.success(),
        "npm install decimal.js failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install_decimal.stdout),
        String::from_utf8_lossy(&install_decimal.stderr)
    );

    // A stub `@cratestack/cbor`, placed in a real `node_modules` directory
    // that is an ancestor of both `pkg/src/runtime.ts` (which bare-imports
    // "@cratestack/cbor") and this test's own driver script below, so
    // Node's module resolution walk-up finds the SAME resolved file (and
    // therefore the same module-level `calls`/`rejectNext` state) from
    // both import sites.
    let stub_dir = root.path().join("node_modules/@cratestack/cbor");
    std::fs::create_dir_all(&stub_dir).expect("create stub dir");
    std::fs::write(
        stub_dir.join("package.json"),
        r#"{ "name": "@cratestack/cbor", "version": "0.0.0-stub", "type": "module", "main": "./index.mjs", "exports": { ".": "./index.mjs" } }"#,
    )
    .expect("write stub package.json");
    std::fs::write(
        stub_dir.join("index.mjs"),
        r#"
let calls = 0;
let rejectNext = false;

export function __getCalls() {
  return calls;
}

export function __setRejectNext(value) {
  rejectNext = value;
}

export async function createCborCodec() {
  calls++;
  if (rejectNext) {
    rejectNext = false;
    throw new Error("stub-induced rejection");
  }
  return {
    contentType: "application/x-stub-cbor",
    encode(value) {
      return new TextEncoder().encode(JSON.stringify(value ?? null));
    },
    decode(bytes) {
      return bytes.length ? JSON.parse(new TextDecoder().decode(bytes)) : undefined;
    },
  };
}
"#,
    )
    .expect("write stub index.mjs");

    // `.mts` (not `.ts`) so tsx treats it as ESM unconditionally — this
    // temp root has no package.json of its own to declare `"type":
    // "module"`, unlike the generated package.json one level down.
    let smoke_path = root.path().join("smoke.mts");
    std::fs::write(
        &smoke_path,
        r#"
import { CratestackRpcRuntime } from "./pkg/src/runtime.js";
import { __getCalls, __setRejectNext } from "@cratestack/cbor";

const stubFetch: typeof fetch = async () => new Response(null, { status: 204 });

const runtime = new CratestackRpcRuntime("http://stub.invalid", { fetch: stubFetch });
await runtime.call("procedure.echoName", null);
await runtime.call("procedure.echoName", null);
await runtime.call("procedure.echoName", null);
if (__getCalls() !== 1) {
  throw new Error(
    `expected exactly 1 createCborCodec() call after 3 runtime.call()s (memoization), got ${__getCalls()}`,
  );
}

__setRejectNext(true);
const runtime2 = new CratestackRpcRuntime("http://stub.invalid", { fetch: stubFetch });

let firstCallThrew = false;
try {
  await runtime2.call("procedure.echoName", null);
} catch {
  firstCallThrew = true;
}
if (!firstCallThrew) {
  throw new Error("expected the first call on runtime2 to throw when the codec factory rejects");
}
if (__getCalls() !== 2) {
  throw new Error(`expected createCborCodec() to have been called twice (1 + rejected), got ${__getCalls()}`);
}

// The critical assertion (issue #746 finding #1): a second call after a
// rejection must retry the factory instead of replaying the stale
// rejection forever.
await runtime2.call("procedure.echoName", null);
if (__getCalls() !== 3) {
  throw new Error(
    `expected createCborCodec() to be retried (3rd invocation) after the prior rejection, got ${__getCalls()}`,
  );
}

console.log("NATIVE_CBOR_MEMOIZATION_AND_RETRY_OK");
"#,
    )
    .expect("write smoke script");

    let mut tsx = tsx_command(root.path(), "smoke.mts");
    let output = tsx.output().expect("run tsx");

    assert!(
        output.status.success(),
        "generated runtime's codec memoization/retry behavior failed under a real Node run:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("NATIVE_CBOR_MEMOIZATION_AND_RETRY_OK"),
        "smoke script did not print its success marker:\n{}",
        command_report(&tsx, &output)
    );
}

/// The constructor must stay synchronous under the flag — see the issue's
/// own "Technical Context" section: an async `static create()` factory was
/// explicitly rejected because it breaks every existing consumer's
/// construction call.
#[test]
fn the_constructor_stays_synchronous_under_the_flag() {
    let native = generate(RPC_FIXTURE, TypeScriptGeneratorConfig::default());
    let runtime = file(&native, "src/runtime.ts");

    assert!(
        runtime.contains("constructor(origin: string, options: CratestackRpcClientOptions = {}) {"),
        "the constructor's signature must remain a plain, synchronous constructor:\n{runtime}"
    );
    assert!(
        !runtime.contains("async constructor"),
        "TypeScript doesn't even allow `async constructor`, but assert directly against \
         reintroducing one by accident:\n{runtime}"
    );
}

#[test]
fn the_flag_is_additive_only_package_json_and_runtime_differ_on_rpc() {
    let plain = generate(
        RPC_FIXTURE,
        TypeScriptGeneratorConfig {
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
    );
    let native = generate(RPC_FIXTURE, TypeScriptGeneratorConfig::default());

    assert_eq!(
        plain.files.len(),
        native.files.len(),
        "native_cbor must not add or remove files, only change contents"
    );

    for plain_file in &plain.files {
        let counterpart = native
            .files
            .iter()
            .find(|candidate| candidate.file_name == plain_file.file_name)
            .unwrap_or_else(|| panic!("native_cbor: true dropped {}", plain_file.file_name));
        if matches!(
            plain_file.file_name.as_str(),
            "package.json" | "src/runtime.ts"
        ) {
            assert_ne!(
                plain_file.contents, counterpart.contents,
                "{} was expected to differ under native_cbor: true but didn't",
                plain_file.file_name
            );
            continue;
        }
        assert_eq!(
            plain_file.contents, counterpart.contents,
            "native_cbor: true changed {} — it must only touch package.json and \
             src/runtime.ts",
            plain_file.file_name
        );
    }
}

#[test]
fn the_flag_has_no_effect_at_all_on_a_rest_transport_schema() {
    let plain = generate(
        REST_FIXTURE,
        TypeScriptGeneratorConfig {
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
    );
    let native = generate(REST_FIXTURE, TypeScriptGeneratorConfig::default());

    assert_eq!(plain.files.len(), native.files.len());
    for plain_file in &plain.files {
        let counterpart = native
            .files
            .iter()
            .find(|candidate| candidate.file_name == plain_file.file_name)
            .unwrap_or_else(|| panic!("native_cbor: true dropped {}", plain_file.file_name));
        assert_eq!(
            plain_file.contents, counterpart.contents,
            "REST: native_cbor must be a true no-op — {} differed",
            plain_file.file_name
        );
    }
}

/// Cratestack#765: `--swr` on an RPC schema renders `src/swr/runtime.ts`
/// from the same `rpc-runtime.ts.j2` template as `src/runtime.ts`, but
/// through `SwrSchemaContext` — a context that used to have no
/// `native_cbor` field, so every `{% if native_cbor %}` site in the shared
/// template silently evaluated falsy (minijinja's `UndefinedBehavior::
/// Lenient`) regardless of the actual flag. Mirrors
/// `default_config_uses_native_cbor` above, scoped to the `swr` layout's
/// own runtime file.
#[test]
fn swr_runtime_honours_native_cbor_default() {
    let config = TypeScriptGeneratorConfig {
        swr: true,
        ..TypeScriptGeneratorConfig::default()
    };
    let native = generate(RPC_FIXTURE, config);

    let runtime = file(&native, "src/swr/runtime.ts");
    assert!(
        runtime.contains("import { createCborCodec } from \"@cratestack/cbor\";"),
        "swr: default config's src/swr/runtime.ts must import createCborCodec:\n{runtime}"
    );
    assert!(
        !runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "swr: default config's src/swr/runtime.ts must not default to jsonRpcCodec:\n{runtime}"
    );
}

/// Mirrors `no_native_cbor_falls_back_to_json_rpc_codec` above, scoped to
/// the `swr` layout's own runtime file — cratestack#765.
#[test]
fn swr_runtime_falls_back_to_json_rpc_codec_when_native_cbor_is_off() {
    let plain = generate(
        RPC_FIXTURE,
        TypeScriptGeneratorConfig {
            swr: true,
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
    );

    let runtime = file(&plain, "src/swr/runtime.ts");
    assert!(
        runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "swr: native_cbor: false src/swr/runtime.ts must default to jsonRpcCodec:\n{runtime}"
    );
    assert!(
        !runtime.contains("@cratestack/cbor"),
        "swr: native_cbor: false src/swr/runtime.ts must not mention @cratestack/cbor:\n{runtime}"
    );
    assert!(
        !runtime.contains("createCborCodec"),
        "swr: native_cbor: false src/swr/runtime.ts must not reference createCborCodec:\n{runtime}"
    );
}

/// The decisive regression test for cratestack#765: `src/runtime.ts` (the
/// default layout) and `src/swr/runtime.ts` (the `--swr` layout) render
/// from the exact same `rpc-runtime.ts.j2` template
/// (`crate::swr::templates::RPC`'s `rpc-runtime.ts.j2` entry), through two
/// independently-maintained context structs (`TemplateContext` vs
/// `SwrSchemaContext`). They must therefore agree on every line except the
/// one deliberate, Rust-computed difference between the two layouts —
/// `models_import_path` (`"./models.js"` vs `"../models.js"`, cratestack#764)
/// — for BOTH `native_cbor` states. This is a stronger guard than checking
/// `native_cbor` alone: it fails on ANY future field the default layout's
/// context gains that `SwrSchemaContext` doesn't mirror, not just this one.
///
/// Confirmed to actually fail: reverting the `native_cbor: config.
/// native_cbor` line this ticket added to `swr::context::build_shared_context`
/// makes this test (and the two above) fail — `src/swr/runtime.ts` keeps
/// emitting the `jsonRpcCodec` fallback regardless of the flag, while
/// `src/runtime.ts` correctly switches — verified by hand while writing
/// this test, not asserted from reading the diff alone.
#[test]
fn swr_and_default_runtimes_agree_on_codec_in_both_flag_states() {
    for native_cbor in [true, false] {
        let config = TypeScriptGeneratorConfig {
            swr: true,
            native_cbor,
            ..TypeScriptGeneratorConfig::default()
        };
        let package = generate(RPC_FIXTURE, config);

        let default_runtime = file(&package, "src/runtime.ts");
        let swr_runtime = file(&package, "src/swr/runtime.ts");

        // The only sanctioned difference between the two layouts: the
        // relative import depth to the shared `src/models.ts` (see
        // `TemplateContext::models_import_path`'s doc comment).
        let normalized_swr_runtime = swr_runtime.replace("../models.js", "./models.js");

        assert_eq!(
            default_runtime, normalized_swr_runtime,
            "native_cbor: {native_cbor} — src/runtime.ts and src/swr/runtime.ts (after \
             normalizing the sanctioned ../models.js -> ./models.js import-depth difference) \
             must be byte-identical; any other divergence means SwrSchemaContext is missing a \
             field TemplateContext has (cratestack#765)"
        );
    }
}

fn generate(fixture_stem: &str, config: TypeScriptGeneratorConfig) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(&schema, &config)
        .unwrap_or_else(|error| panic!("{fixture_stem}: generation should succeed: {error}"))
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
        .contents
        .as_str()
}
