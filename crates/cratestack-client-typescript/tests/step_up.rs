//! cratestack#1244: a step-up refusal reaches both generated TypeScript
//! clients typed as `stepUp` (and `stepUpOf`, which also reads a batch
//! frame's error body), and `retryAfterStepUp` retries a call at most once
//! after a re-proof.
//!
//! The structural test always runs; the Node-driven one runs the generated
//! clients over a scripted `fetch` (skips, printed, when `node`/`npm` are not
//! on `PATH`, like `auth_refusal.rs`).

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

fn generate(fixture: &str, package_name: &str) -> GeneratedTypeScriptPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path:?} should parse: {error}"));
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
            native_cbor: false,
            rtk: false,
        },
    )
    .expect("default template should render")
}

fn runtime(package: &GeneratedTypeScriptPackage) -> &str {
    package
        .files
        .iter()
        .find(|file| file.file_name == "src/runtime.ts")
        .expect("src/runtime.ts")
        .contents
        .as_str()
}

#[test]
fn both_runtimes_export_the_step_up_surface() {
    for (fixture, name, test) in [
        ("tiny_rest", "tiny-rest-client", "this.status === 401"),
        (
            "tiny_rpc",
            "tiny-rpc-client",
            r#"body.code === "unauthenticated""#,
        ),
    ] {
        let package = generate(fixture, name);
        let runtime = runtime(&package);
        for needle in [
            "export interface StepUpRequired {",
            "readonly stepUp: StepUpRequired | undefined;",
            "export function stepUpOf(error: unknown): StepUpRequired | undefined {",
            "export async function retryAfterStepUp<T>(",
            test,
        ] {
            assert!(
                runtime.contains(needle),
                "{fixture} runtime lacks `{needle}`"
            );
        }
    }
}

/// The smoke script: a `fetch` that answers each call with the next scripted
/// answer, then every case, each printed as one `CASE <name> <json>` line.
fn smoke(class: &str, code: &str) -> String {
    format!(
        r#"
import {{ {class}, retryAfterStepUp, stepUpOf }} from "./src/index";

const refusal = (details: unknown, error = "insufficient_user_authentication", code = "{code}") =>
  ({{ status: 401, body: {{ code, message: "step-up authentication required", details, error }} }});
const required = {{ max_age: 300, acr_values: ["aal2", "aal3"], unmet: ["max_age", "acr"] }};

function client(answers: {{ status: number; body: unknown }}[]) {{
  const state = {{ sent: 0 }};
  const fetch = async () => {{
    const {{ status, body }} = answers[state.sent++]!;
    return new Response(JSON.stringify(body), {{ status, headers: {{ "content-type": "application/json" }} }});
  }};
  return {{ state, api: new {class}("http://127.0.0.1:9", {{ basePath: "/api", fetch: fetch as typeof globalThis.fetch }}) }};
}}

async function failure(answers: {{ status: number; body: unknown }}[]): Promise<any> {{
  try {{
    await client(answers).api.procedures.echoName({{ name: "x" }});
  }} catch (error) {{
    return error;
  }}
  throw new Error("the call did not fail");
}}

const out = (name: string, value: unknown) => console.log("CASE " + name + " " + JSON.stringify(value));

out("typed", (await failure([refusal(required)])).stepUp);
out("plain", (await failure([{{ status: 401, body: {{ code: "{code}", message: "no" }} }}])).stepUp ?? null);
out("other", (await failure([refusal(required, "invalid_token")])).stepUp ?? null);
out("mistyped", (await failure([refusal({{ max_age: "300", acr_values: "aal2", amr: [1, "pwd"], unmet: {{}} }})])).stepUp);
out("not-object", stepUpOf(await failure([refusal("nope")])));
out("frame", stepUpOf({{ code: "unauthenticated", message: "m", details: {{ amr: ["hwk"] }}, error: "insufficient_user_authentication" }}) ?? null);

{{
  const {{ state, api }} = client([refusal(required), {{ status: 200, body: "ok" }}]);
  const result = await retryAfterStepUp(() => api.procedures.echoName({{ name: "x" }}), async () => true);
  out("retried", {{ result, sent: state.sent }});
}}
{{
  const {{ state, api }} = client([refusal(required), refusal(required), {{ status: 200, body: "ok" }}]);
  let reproofs = 0;
  const error: any = await retryAfterStepUp(() => api.procedures.echoName({{ name: "x" }}), async () => {{
    reproofs++;
    return true;
  }}).catch((e) => e);
  out("twice", {{ stepUp: error.stepUp !== undefined, sent: state.sent, reproofs }});
}}
{{
  const {{ state, api }} = client([refusal(required)]);
  const error: any = await retryAfterStepUp(() => api.procedures.echoName({{ name: "x" }}), async () => false).catch((e) => e);
  out("declined", {{ stepUp: error.stepUp !== undefined, sent: state.sent }});
}}
"#
    )
}

const TYPED: &str =
    r#"{"maxAge":300,"acrValues":["aal2","aal3"],"amr":[],"unmet":["max_age","acr"]}"#;
const EMPTY: &str = r#"{"acrValues":[],"amr":["pwd"],"unmet":[]}"#;

fn run(fixture: &str, package_name: &str, class: &str, code: &str, frame: &str) {
    let package = generate(fixture, package_name);
    let dir = tempfile::tempdir().expect("tempdir");
    for generated in &package.files {
        let path = dir.path().join(&generated.file_name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, &generated.contents).expect("write");
    }
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
    std::fs::write(dir.path().join("smoke.ts"), smoke(class, code)).expect("write smoke");
    let mut tsx = tsx_command(dir.path(), "smoke.ts");
    let output = tsx.output().expect("run tsx");
    assert!(
        output.status.success(),
        "smoke script failed:\n{}",
        command_report(&tsx, &output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let case = |name: &str| -> String {
        stdout
            .lines()
            .find_map(|line| line.strip_prefix(&format!("CASE {name} ")))
            .unwrap_or_else(|| panic!("no `{name}` line:\n{}", command_report(&tsx, &output)))
            .to_owned()
    };
    assert_eq!(case("typed"), TYPED, "{fixture}");
    assert_eq!(case("plain"), "null", "{fixture}");
    assert_eq!(case("other"), "null", "{fixture}");
    assert_eq!(case("mistyped"), EMPTY, "{fixture}");
    assert_eq!(
        case("not-object"),
        r#"{"acrValues":[],"amr":[],"unmet":[]}"#,
        "{fixture}"
    );
    assert_eq!(case("frame"), frame, "{fixture}");
    assert_eq!(case("retried"), r#"{"result":"ok","sent":2}"#, "{fixture}");
    assert_eq!(
        case("twice"),
        r#"{"stepUp":true,"sent":2,"reproofs":1}"#,
        "{fixture}"
    );
    assert_eq!(case("declined"), r#"{"stepUp":true,"sent":1}"#, "{fixture}");
}

#[test]
fn a_step_up_reaches_both_generated_clients_typed_and_is_retried_once() {
    if !node_toolchain_available() {
        eprintln!("skipping a step-up smoke: `node`/`npm` not on PATH");
        return;
    }
    run(
        "tiny_rest",
        "tiny-rest-client",
        "TinyRestClientClient",
        "UNAUTHORIZED",
        // The REST `stepUpOf` reads a `CratestackHttpError` only.
        "null",
    );
    run(
        "tiny_rpc",
        "tiny-rpc-client",
        "TinyRpcClientClient",
        "unauthenticated",
        r#"{"acrValues":[],"amr":["hwk"],"unmet":[]}"#,
    );
}
