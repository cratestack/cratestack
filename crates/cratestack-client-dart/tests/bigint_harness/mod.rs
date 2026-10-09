//! Shared harness for the `BigInt` round-trip proofs: generate a real Dart
//! package, resolve it, expand its `build_runner` parts, and run a Dart test
//! file in it on a chosen runtime.
//!
//! The runtimes are the point. `Vm` is `flutter test`; `Dart2Js` and
//! `Dart2Wasm` are `flutter test --platform chrome` with and without `--wasm`.
//! A `BigInt` decoded through `int` is exact on the first and the third and
//! wrong on the second, so a suite that only ran on the VM could not tell the
//! right decode from the wrong one.
//!
//! Each web runtime needs a Chrome and, for wasm, a Flutter that knows
//! `--wasm`. The `cratestack_cbor` native codec needs its vendored library,
//! which the published package ships for dev-mode `flutter test` only on
//! linux-x64 (macOS and Windows have it only inside an app bundle); anywhere
//! else `CRATESTACK_CBOR_NATIVE_LIB` has to name a built one. When a runtime
//! is missing its leg is skipped with a printed line, as the rest of this
//! crate's `flutter` tests do when `flutter` is absent; set
//! `CRATESTACK_REQUIRE_DART_RUNTIMES=1` to turn that skip into a failure (what
//! a job that provisions these should do, so a quiet skip cannot pass for a
//! green run).
//!
//! The `riverpod` preset pins `build_runner`, `riverpod_generator` and
//! `dart_mappable_builder` as one unit against Flutter stable. A Flutter whose
//! `flutter_test` those pins cannot be solved against fails `flutter pub get`
//! before any BigInt code runs; that leg then skips with the reason, or, with
//! `CRATESTACK_RELAX_RIVERPOD_PINS=1`, runs against a scratch pubspec whose
//! pins are `any`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

/// Every `flutter` and `dart` invocation in this binary takes this lock.
///
/// The tests here run in parallel threads, and two Flutter tool processes
/// started together intermittently failed with "the Flutter SDK is not
/// available": `flutter pub get` (behind its "startup lock" message), `dart run
/// build_runner`, and a `flutter test` starting while another process updates
/// the SDK's cache. Taking turns costs wall time and nothing else, and a flaky
/// harness cannot tell a real BigInt failure from a tooling race.
static FLUTTER_LOCK: Mutex<()> = Mutex::new(());

fn lock_flutter() -> std::sync::MutexGuard<'static, ()> {
    FLUTTER_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runtime {
    Vm,
    Dart2Js,
    Dart2Wasm,
}

impl Runtime {
    pub const ALL: [Runtime; 3] = [Runtime::Vm, Runtime::Dart2Js, Runtime::Dart2Wasm];

    fn label(self) -> &'static str {
        match self {
            Runtime::Vm => "Dart VM",
            Runtime::Dart2Js => "dart2js (chrome)",
            Runtime::Dart2Wasm => "dart2wasm (chrome --wasm)",
        }
    }

    fn flutter_args(self) -> &'static [&'static str] {
        match self {
            Runtime::Vm => &[],
            Runtime::Dart2Js => &["--platform", "chrome"],
            Runtime::Dart2Wasm => &["--platform", "chrome", "--wasm"],
        }
    }
}

pub fn flutter_available() -> bool {
    let _flutter = lock_flutter();
    Command::new("flutter")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// `CRATESTACK_RELAX_RIVERPOD_PINS=1`: see [`Package::resolve_and_build`].
fn relax_riverpod_pins() -> bool {
    std::env::var_os("CRATESTACK_RELAX_RIVERPOD_PINS").is_some_and(|value| value != "0")
}

pub fn require_runtimes() -> bool {
    std::env::var_os("CRATESTACK_REQUIRE_DART_RUNTIMES").is_some_and(|value| value != "0")
}

/// `Ok` when `cratestack_cbor`'s native backend can load on this host under
/// `flutter test`; `Err(reason)` otherwise. See the module doc.
pub fn native_cbor_available() -> Result<(), String> {
    let overridden = std::env::var_os("CRATESTACK_CBOR_NATIVE_LIB")
        .is_some_and(|path| Path::new(&path).is_file());
    if overridden || cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Ok(());
    }
    Err(
        "cratestack_cbor ships a dev-mode native library only for linux-x64; \
         set CRATESTACK_CBOR_NATIVE_LIB to a built libcratestack_client_flutter"
            .to_owned(),
    )
}

/// `CHROME_EXECUTABLE` if it points at a file, else the usual install paths.
fn chrome_executable() -> Option<PathBuf> {
    let from_env = std::env::var_os("CHROME_EXECUTABLE").map(PathBuf::from);
    let well_known = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ]
    .map(PathBuf::from);
    from_env
        .into_iter()
        .chain(well_known)
        .find(|path| path.is_file())
}

/// The Flutter SDK root, from `flutter --version --machine`.
fn flutter_root() -> Option<String> {
    static ROOT: OnceLock<Option<String>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let _flutter = lock_flutter();
        let output = Command::new("flutter")
            .args(["--version", "--machine"])
            .output()
            .ok()?;
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        json.get("flutterRoot")?.as_str().map(str::to_owned)
    })
    .clone()
}

fn flutter_knows_wasm() -> bool {
    let _flutter = lock_flutter();
    Command::new("flutter")
        .args(["test", "--help"])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("--wasm"))
}

/// `Ok` when this host can run `runtime`; `Err(reason)` when it cannot.
fn runtime_available(runtime: Runtime) -> Result<(), String> {
    match runtime {
        Runtime::Vm => Ok(()),
        Runtime::Dart2Js | Runtime::Dart2Wasm if chrome_executable().is_none() => {
            Err("no Chrome found (set CHROME_EXECUTABLE to a Chrome or Chromium binary)".to_owned())
        }
        Runtime::Dart2Wasm if !flutter_knows_wasm() => {
            Err("this Flutter's `flutter test` has no --wasm".to_owned())
        }
        _ => Ok(()),
    }
}

pub struct Package {
    pub dir: PathBuf,
    label: String,
    is_riverpod: bool,
}

/// Generates `fixture` as a package named `library_name`, resolves it against
/// this working tree's `dart-packages/`, and expands its builder parts.
pub fn generate(
    label: &str,
    fixture: &str,
    library_name: &str,
    preset: DartPreset,
    native_cbor: bool,
) -> Package {
    let fixture_path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("{fixture_path} should parse: {error}"));
    let generated = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor,
        },
    )
    .expect("default template should render");

    let dir = project_tmp_path(label);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("existing tmp dir should be removable");
    }
    for file in &generated.files {
        let path = dir.join(&file.file_name);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent dir");
        fs::write(&path, &file.contents).expect("write generated file");
    }
    override_in_repo_dart_packages(&dir);
    Package {
        dir,
        label: label.to_owned(),
        is_riverpod: preset == DartPreset::Riverpod,
    }
}

impl Package {
    /// Adds `package:cbor` as a dev dependency. A `native_cbor` package has no
    /// `cbor` dependency of its own, but the echo test decodes the client's
    /// request with it on purpose: an independent decoder, not the codec under
    /// test (the same move `just verify-dart` makes for the native echo tests).
    pub fn add_cbor_dev_dependency(&self) {
        let path = self.dir.join("pubspec.yaml");
        let pubspec = fs::read_to_string(&path).expect("read pubspec.yaml");
        let patched = pubspec.replacen(
            "dev_dependencies:\n",
            "dev_dependencies:\n  cbor: ^6.5.1\n",
            1,
        );
        assert_ne!(
            pubspec, patched,
            "pubspec.yaml has no dev_dependencies block"
        );
        fs::write(&path, patched).expect("write pubspec.yaml");
    }

    /// `cratestack_cbor` is version-locked to this crate's own version, which a
    /// bump PR has not published yet, so resolve it as `any` (what
    /// `verify-dart` does for its native packages). Verification-only.
    pub fn allow_any_cratestack_cbor(&self) {
        let path = self.dir.join("pubspec_overrides.yaml");
        let mut overrides = fs::read_to_string(&path).expect("read pubspec_overrides.yaml");
        overrides.push_str("  cratestack_cbor: any\n");
        fs::write(&path, overrides).expect("write pubspec_overrides.yaml");
    }

    pub fn add_test(&self, source_fixture: &str, as_name: &str) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(source_fixture);
        let target = self.dir.join("test").join(as_name);
        fs::create_dir_all(target.parent().expect("test dir")).expect("create test dir");
        fs::copy(&source, &target)
            .unwrap_or_else(|error| panic!("copy {source:?} to {target:?}: {error}"));
    }

    /// Like [`Package::add_test`], for a test file written against the
    /// package named `from` that is to run in the package named `to`: every
    /// `package:<from>/` and `<from>` class-prefix reference is repointed. The
    /// echo test is shared between a pure-`package:cbor` and a native package.
    pub fn add_test_with_library(&self, source_fixture: &str, as_name: &str, from: &str, to: &str) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(source_fixture);
        let text =
            fs::read_to_string(&source).unwrap_or_else(|error| panic!("read {source:?}: {error}"));
        let rewritten = text
            .replace(
                &format!("package:{from}/{from}.dart"),
                &format!("package:{to}/{to}.dart"),
            )
            .replace(&pascal_client(from), &pascal_client(to));
        assert_ne!(
            text, rewritten,
            "{source_fixture} does not mention package {from}"
        );
        let target = self.dir.join("test").join(as_name);
        fs::create_dir_all(target.parent().expect("test dir")).expect("create test dir");
        fs::write(&target, rewritten).expect("write test file");
    }

    /// `flutter pub get` then the `build_runner` pass `models.dart`'s
    /// `part 'models.builder.dart'` needs before anything compiles.
    ///
    /// `Err(reason)` only for the one environmental failure the `riverpod`
    /// preset has on a Flutter channel its pinned `riverpod_generator` trio was
    /// not solved against (see [`Package::resolve_and_build`]'s body); every
    /// other failure panics.
    pub fn resolve_and_build(&self) -> Result<(), String> {
        // Resolved before the lock is taken: `flutter_root` takes it itself.
        let flutter_root = flutter_root();
        let _flutter = lock_flutter();
        if self.is_riverpod && relax_riverpod_pins() {
            self.relax_riverpod_pins_in_pubspec();
        }
        let pub_get = Command::new("flutter")
            .args(["pub", "get"])
            .current_dir(&self.dir)
            .output()
            .expect("run flutter pub get");
        if !pub_get.status.success() {
            let output = format!(
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&pub_get.stdout),
                String::from_utf8_lossy(&pub_get.stderr)
            );
            // The generated riverpod pubspec pins build_runner, riverpod_generator
            // and dart_mappable_builder as one unit against Flutter stable's
            // flutter_test; another channel can make that unsolvable, which is
            // an environment fact, not a BigInt result.
            if self.is_riverpod && output.contains("version solving failed") {
                return Err(
                    "`flutter pub get` cannot solve the riverpod preset's pinned \
                     riverpod_generator/build_runner on this Flutter channel (the same \
                     failure as computed_params_deep_equality_holds_under_riverpod_preset); \
                     rerun with CRATESTACK_RELAX_RIVERPOD_PINS=1 to loosen the pins in this \
                     test's scratch copy only"
                        .to_owned(),
                );
            }
            panic!("flutter pub get failed:\n{output}");
        }
        // `dart run` re-resolves on its own when it judges `pubspec.lock` newer
        // than `package_config.json`, and a bare `dart` cannot resolve
        // `sdk: flutter` without `FLUTTER_ROOT` ("the Flutter SDK is not
        // available"). Concurrent packages made that timestamp race lose once,
        // so say where Flutter is.
        let mut build_runner = Command::new("dart");
        build_runner
            .args([
                "run",
                "build_runner",
                "build",
                "--delete-conflicting-outputs",
            ])
            .current_dir(&self.dir);
        if let Some(root) = flutter_root {
            build_runner.env("FLUTTER_ROOT", root);
        }
        let output = build_runner.output().expect("run dart run build_runner");
        assert!(
            output.status.success(),
            "dart run build_runner build failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    /// Loosens the riverpod preset's exact pins to `any` in this scratch copy
    /// of the pubspec, so a Flutter channel newer than stable can resolve it.
    /// The generated `pubspec.yaml` the generator emits is not touched.
    fn relax_riverpod_pins_in_pubspec(&self) {
        let path = self.dir.join("pubspec.yaml");
        let pubspec = fs::read_to_string(&path).expect("read pubspec.yaml");
        let relaxed = pubspec
            .replace("riverpod_annotation: 4.0.3", "riverpod_annotation: any")
            .replace("riverpod_generator: 4.0.4", "riverpod_generator: any")
            .replace("dart_mappable_builder: 4.8.0", "dart_mappable_builder: any")
            .replace("build_runner: \">=2.14.0 <2.15.2\"", "build_runner: any");
        assert_ne!(pubspec, relaxed, "no riverpod pins found to relax");
        fs::write(&path, relaxed).expect("write pubspec.yaml");
    }

    /// `flutter analyze --fatal-warnings --no-fatal-infos`, the gate
    /// `just verify-dart` puts on every generated package: a `BigInt` arm that
    /// compiled by luck but emits an unused import, a needless cast or an
    /// undefined name fails here rather than in a consumer's analyzer.
    pub fn analyze(&self) {
        let _flutter = lock_flutter();
        let output = Command::new("flutter")
            .args(["analyze", "--fatal-warnings", "--no-fatal-infos"])
            .current_dir(&self.dir)
            .output()
            .expect("run flutter analyze");
        assert!(
            output.status.success(),
            "flutter analyze found issues in the generated {} package:\nstdout: {}\nstderr: {}",
            self.label,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!(
            "ANALYZED [{}]: flutter analyze --fatal-warnings clean",
            self.label
        );
    }

    /// Runs `test_file` on `runtime`. Returns the Dart runner's own success
    /// line, or `None` when the host cannot run that runtime (a printed skip).
    pub fn run_on(&self, test_file: &str, runtime: Runtime) -> Option<String> {
        if let Err(reason) = runtime_available(runtime) {
            assert!(
                !(require_runtimes() && runtime != Runtime::Vm),
                "CRATESTACK_REQUIRE_DART_RUNTIMES is set but {} cannot run here: {reason}",
                runtime.label()
            );
            eprintln!(
                "SKIPPED [{}] {test_file} on {}: {reason}",
                self.label,
                runtime.label()
            );
            return None;
        }
        let _flutter = lock_flutter();
        let mut command = Command::new("flutter");
        command
            .arg("test")
            .args(runtime.flutter_args())
            .arg(format!("test/{test_file}"))
            .current_dir(&self.dir);
        if let Some(chrome) = chrome_executable() {
            command.env("CHROME_EXECUTABLE", chrome);
        }
        let output = command.output().expect("run flutter test");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("All tests passed!"),
            "{test_file} FAILED on {}:\nstdout: {stdout}\nstderr: {stderr}",
            runtime.label()
        );
        let line = stdout
            .lines()
            .rev()
            .find(|line| line.contains("All tests passed!"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        eprintln!(
            "RAN [{}] {test_file} on {}: {line}",
            self.label,
            runtime.label()
        );
        Some(line)
    }

    /// Removes the package, unless `CRATESTACK_KEEP_DART_TMP` is set (to rerun
    /// a Dart test by hand with `flutter test` inside it).
    pub fn cleanup(self) {
        if std::env::var_os("CRATESTACK_KEEP_DART_TMP").is_some() {
            eprintln!("kept {}", self.dir.display());
            return;
        }
        fs::remove_dir_all(&self.dir).expect("tmp dir should be removable");
    }
}

/// `bigint_round_trip_check` becomes `BigintRoundTripCheckCratestackClient`,
/// the name the generator gives the client class.
fn pascal_client(library_name: &str) -> String {
    let mut name: String = library_name
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    name.push_str("CratestackClient");
    name
}

fn project_tmp_path(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/client-dart-tests")
        .join(format!("{label}-{suffix}"))
}

/// Points `cratestack_builder` and `cratestack_annotations` at this repo's
/// own `dart-packages/`, so `pub get` resolves the working tree's constraints
/// rather than the last published release's (see `decimal_round_trip.rs`).
fn override_in_repo_dart_packages(dir: &Path) {
    let dart_packages = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dart-packages")
        .canonicalize()
        .expect("dart-packages/ should exist in this repo");
    fs::write(
        dir.join("pubspec_overrides.yaml"),
        format!(
            "dependency_overrides:\n  \
             cratestack_annotations:\n    path: {0}/cratestack_annotations\n  \
             cratestack_builder:\n    path: {0}/cratestack_builder\n",
            dart_packages.display()
        ),
    )
    .expect("write pubspec_overrides.yaml");
}
