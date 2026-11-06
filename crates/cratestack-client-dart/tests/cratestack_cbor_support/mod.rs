//! What a test needs to resolve and run a generated package with `flutter`,
//! now that every generated package depends on `cratestack_cbor` (the
//! pure-Dart `package:cbor` codec mode is gone).
//!
//! Shared by every test binary that shells out to `flutter`, so the
//! override file and the native-library lookup are written once:
//!
//! * [`write_pubspec_overrides`] writes the package's single
//!   `pubspec_overrides.yaml` (pub reads one per package): `cratestack_cbor`
//!   as `any` or as a local stub, plus this repo's own
//!   `cratestack_annotations`/`cratestack_builder`.
//! * [`point_at_native_lib`] sets `CRATESTACK_CBOR_NATIVE_LIB` on a
//!   `flutter test` command, to the vendored library `flutter pub get`
//!   fetched.
//! * [`add_cbor_dev_dependency`] gives a test file an independent CBOR
//!   decoder (`package:cbor`) to judge the generated codec against.
//!
//! The same two steps as `justfile`'s `override_cratestack_cbor_for_verification`
//! and `native_cbor_vendored_lib_for`, which do this for `just verify-dart`.

// Each test binary compiles its own copy of this module and uses only part
// of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `cratestack_cbor`'s escape hatch for naming its native library directly
/// (`dart-packages/cratestack_cbor/lib/src/native/native_cbor_codec.dart`).
pub const NATIVE_LIB_ENV: &str = "CRATESTACK_CBOR_NATIVE_LIB";

/// Where the generated package's `cratestack_cbor` dependency resolves from.
pub enum CratestackCbor<'a> {
    /// pub.dev, as `any`. The generated requirement is an API floor with a
    /// release-line ceiling (cratestack#779), so it normally resolves as
    /// written; `any` keeps a release PR whose floor or ceiling has moved
    /// ahead of what pub.dev serves from failing `pub get` on a version the
    /// release itself is about to publish. What these tests verify needs a
    /// published `cratestack_cbor`, not that exact one. The emitted text is
    /// pinned separately by `tests/native_cbor_generator.rs`.
    Published,
    /// A local package, such as a stub whose codec factory fails on demand.
    Path(&'a Path),
}

/// Writes `package_dir/pubspec_overrides.yaml`.
///
/// An overrides file rather than an edit to the generated `pubspec.yaml`:
/// the generated file stays byte-for-byte what the generator emitted, so
/// nothing here can mask a regression in it. `cratestack_annotations` and
/// `cratestack_builder` point at this repo's `dart-packages/`, so `pub get`
/// resolves the working tree's constraints rather than the last published
/// release's: a commit that raises the generator's annotations floor and the
/// builder's own `cratestack_annotations` constraint together cannot resolve
/// otherwise (see `package_floors.rs`'s module doc on that chicken-and-egg).
pub fn write_pubspec_overrides(package_dir: &Path, cbor: CratestackCbor<'_>) {
    let dart_packages = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dart-packages")
        .canonicalize()
        .expect("dart-packages/ should exist in this repo");
    let cratestack_cbor = match cbor {
        CratestackCbor::Published => "any".to_owned(),
        CratestackCbor::Path(path) => {
            let path = path
                .canonicalize()
                .unwrap_or_else(|error| panic!("cratestack_cbor override {path:?}: {error}"));
            format!("\n    path: {}", path.display())
        }
    };
    fs::write(
        package_dir.join("pubspec_overrides.yaml"),
        format!(
            "dependency_overrides:\n  \
             cratestack_cbor: {cratestack_cbor}\n  \
             cratestack_annotations:\n    path: {0}/cratestack_annotations\n  \
             cratestack_builder:\n    path: {0}/cratestack_builder\n",
            dart_packages.display()
        ),
    )
    .expect("write pubspec_overrides.yaml");
}

/// Adds `package:cbor` to the generated package's `dev_dependencies`, for a
/// test file that decodes what the client sent with an independent decoder
/// rather than with the codec under test.
pub fn add_cbor_dev_dependency(package_dir: &Path) {
    let path = package_dir.join("pubspec.yaml");
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

/// The native library `flutter pub get` fetched with `cratestack_cbor`, read
/// from `.dart_tool/package_config.json`. `None` before `pub get`, and on any
/// host but linux-x64: the published package carries a dev-mode library for
/// that host only (elsewhere it lives inside an app bundle).
pub fn vendored_native_lib(package_dir: &Path) -> Option<PathBuf> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return None;
    }
    let config_dir = package_dir.join(".dart_tool");
    let config = fs::read_to_string(config_dir.join("package_config.json")).ok()?;
    let config: serde_json::Value = serde_json::from_str(&config).ok()?;
    let root_uri = config
        .get("packages")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("name").and_then(|name| name.as_str()) == Some("cratestack_cbor"))?
        .get("rootUri")?
        .as_str()?;
    // An absolute `file://` URI for a hosted package; relative to
    // `.dart_tool/` for a `path:` one.
    let root = match root_uri.strip_prefix("file://") {
        Some(path) => PathBuf::from(percent_decode(path)),
        None => config_dir.join(percent_decode(root_uri)),
    };
    let library = root.join("blobs/linux-x64/libcratestack_client_flutter.so");
    library.is_file().then_some(library)
}

/// Sets [`NATIVE_LIB_ENV`] on `command` to [`vendored_native_lib`], unless
/// the caller's environment already names a library (that one wins, so a
/// host without a dev-mode library can point at a self-built one).
///
/// `cratestack_cbor` can also find the library itself under `flutter test`
/// (cratestack#794), but naming it removes that lookup from what a failing
/// run could be blamed on, the same choice `just verify-dart` makes.
pub fn point_at_native_lib<'a>(command: &'a mut Command, package_dir: &Path) -> &'a mut Command {
    if std::env::var_os(NATIVE_LIB_ENV).is_some_and(|value| !value.is_empty()) {
        return command;
    }
    if let Some(library) = vendored_native_lib(package_dir) {
        command.env(NATIVE_LIB_ENV, library);
    }
    command
}

/// `%XX` escapes in a `package_config.json` URI (a space in a pub-cache path,
/// say). Anything malformed is kept as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}
