// Behavioral + snapshot tests for issue #302's per-operation `@riverpod`
// providers. Complements `just verify-dart` (the real `flutter pub get`
// -> `dart run build_runner build` -> `flutter analyze` -> `flutter test`
// pipeline, which is the load-bearing proof these providers actually
// compile and that the override-propagation test actually passes): the
// assertions here are the fast, Rust-side regression guard for the same
// properties — collision-free naming, routing exclusively through the
// existing per-model `Provider<XApi>`, and the riverpod-only pubspec
// additions staying off the `default` preset.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

fn generate(fixture: &str, library_name: &str, preset: DartPreset) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            pb_lock: None,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
        },
    )
    .unwrap_or_else(|error| panic!("{fixture} should generate under {preset:?}: {error}"))
}

fn package_file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("missing generated file {name}\n{:#?}", file_names(package)))
        .contents
        .as_str()
}

fn file_names(package: &GeneratedDartPackage) -> Vec<&str> {
    package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect()
}

#[test]
fn every_model_operation_gets_a_provider_built_on_the_existing_api_provider() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Riverpod);
    let widget = package_file(&package, "lib/src/models/widget.dart");

    // Reads: functions.
    assert!(
        widget.contains("Future<Widget> widget(Ref ref, int id) {\n  return ref.watch(tinyRpcClientWidgetApiProvider).get(id);\n}"),
        "get provider missing or not built on the existing WidgetApi provider:\n{widget}"
    );
    assert!(
        widget.contains("Future<IList<Widget>> widgetList(Ref ref) {\n  return ref.watch(tinyRpcClientWidgetApiProvider).list();\n}"),
        "list provider missing or not built on the existing WidgetApi provider:\n{widget}"
    );

    // Writes: controllers, each reading (not watching) the same existing
    // provider inside their action method. `declared_method` is the
    // controller's own method name; `api_call` is the underlying
    // `WidgetApi` method it calls through to (the update controller's
    // own method is renamed to `save` to avoid colliding with
    // `AsyncNotifier`'s built-in `update(...)` — see
    // `model_providers.dart.j2`'s comment — but it still calls the
    // model API's real `.update(...)` method underneath).
    for (controller, declared_method, api_call) in [
        ("WidgetCreateController", "create", "create"),
        ("WidgetUpdateController", "save", "update"),
        ("WidgetDeleteController", "delete", "delete"),
    ] {
        assert!(
            widget.contains(&format!("class {controller} extends _${controller} {{")),
            "{controller} missing:\n{widget}"
        );
        assert!(
            widget.contains(&format!("Future<Widget> {declared_method}(")),
            "{controller} should declare a `{declared_method}` method:\n{widget}"
        );
        assert!(
            widget.contains(&format!(
                "ref.read(tinyRpcClientWidgetApiProvider).{api_call}("
            )),
            "{controller}'s {declared_method}() should call through tinyRpcClientWidgetApiProvider.{api_call}(...):\n{widget}"
        );
    }

    // Never touches the adapter/client provider directly from one of
    // *this story's new* provider bodies. Scoped to the text after the
    // "Issue #302" marker comment: the pre-existing `Provider<WidgetApi>`
    // above it (relocated by #301) legitimately does
    // `ref.watch(tinyRpcClientClientProvider)` — that's the thing these
    // new providers are supposed to route through instead of
    // reimplementing.
    let new_providers_section = widget
        .split("// Issue #302: one `@riverpod` provider per operation")
        .nth(1)
        .expect("model file should carry the issue #302 provider section");
    assert!(
        !new_providers_section.contains("ref.watch(tinyRpcClientAdapterProvider)")
            && !new_providers_section.contains("ref.read(tinyRpcClientAdapterProvider)"),
        "a generated provider reached the adapter provider directly:\n{new_providers_section}"
    );
    assert!(
        !new_providers_section.contains("ref.watch(tinyRpcClientClientProvider)")
            && !new_providers_section.contains("ref.read(tinyRpcClientClientProvider)"),
        "a generated provider reached the client provider directly:\n{new_providers_section}"
    );
}

#[test]
fn every_procedure_gets_a_provider_shaped_by_its_kind() {
    let package = generate(
        "ci_rpc",
        "dart_verify_riverpod_ci_rpc",
        DartPreset::Riverpod,
    );
    let procedures = package_file(&package, "lib/src/procedures.dart");

    // `listPosts` is a query procedure -> plain function provider.
    assert!(
        procedures.contains("Future<List<Post>> listPosts(Ref ref, ListPostsArgs args) {\n  return ref.watch(dartVerifyRiverpodCiRpcProceduresApiProvider).listPosts(args);\n}"),
        "query procedure provider missing or not built on the existing ProceduresApi provider:\n{procedures}"
    );

    // `currentStatus` is a `mutation procedure` -> controller class.
    assert!(
        procedures.contains("class CurrentStatusController extends _$CurrentStatusController {"),
        "mutation procedure controller missing:\n{procedures}"
    );
    assert!(
        procedures
            .contains("ref.read(dartVerifyRiverpodCiRpcProceduresApiProvider).currentStatus(args)"),
        "mutation procedure controller should call through the existing ProceduresApi provider:\n{procedures}"
    );
}

#[test]
fn model_and_procedure_files_carry_the_part_directive() {
    let package = generate(
        "ci_rpc",
        "dart_verify_riverpod_ci_rpc",
        DartPreset::Riverpod,
    );

    assert!(package_file(&package, "lib/src/models/author.dart").contains("part 'author.g.dart';"));
    assert!(package_file(&package, "lib/src/models/post.dart").contains("part 'post.g.dart';"));
    assert!(
        package_file(&package, "lib/src/procedures.dart").contains("part 'procedures.g.dart';")
    );
    // No `@riverpod` surface lives in these two files, so no `part`
    // directive should appear in them either.
    assert!(!package_file(&package, "lib/src/client.dart").contains("part '"));
    assert!(!package_file(&package, "lib/src/models/shared_types.dart").contains("part '"));
}

/// The naming collision this fixture deliberately constructs (see the
/// `.cstack` file's own header comment): naive per-operation provider
/// names for `Widget.list`, `WidgetList.get`, and the `widgetCreate`
/// mutation procedure would all collide with an existing model's own
/// symbol. Asserts the escalation in `provider_naming.rs` actually fires
/// and produces distinct, deterministic names — `just verify-dart`
/// separately proves those escalated names still compile and pass
/// `flutter analyze`/`build_runner`.
#[test]
fn colliding_provider_names_escalate_to_distinct_symbols() {
    let package = generate(
        "riverpod_provider_collision",
        "dart_verify_riverpod_collision",
        DartPreset::Riverpod,
    );

    let widget = package_file(&package, "lib/src/models/widget.dart");
    let widget_list = package_file(&package, "lib/src/models/widget_list.dart");
    let procedures = package_file(&package, "lib/src/procedures.dart");

    // Widget claims the naive names first (declared first in the schema).
    assert!(widget.contains("Future<Widget> widget(Ref ref, int id)"));
    assert!(widget.contains("Future<IList<Widget>> widgetList(Ref ref)"));
    assert!(widget.contains("class WidgetCreateController extends _$WidgetCreateController {"));

    // WidgetList's own `get` provider wanted the name `widgetList` too —
    // already taken, so it must have escalated to something else, and
    // that something else must actually appear as a real symbol (not
    // just "not the naive name").
    assert!(
        !widget_list.contains("Future<WidgetList> widgetList(Ref ref, int id)"),
        "WidgetList's get provider should not have kept the colliding name:\n{widget_list}"
    );
    assert!(
        widget_list.contains("Future<WidgetList>")
            && widget_list.contains("(Ref ref, int id) {\n  return ref.watch(dartVerifyRiverpodCollisionWidgetListApiProvider).get(id);\n}"),
        "WidgetList's get provider should still exist under an escalated name:\n{widget_list}"
    );

    // The `widgetCreate` mutation procedure wanted `WidgetCreateController`
    // too — already taken by the Widget model, so it must have escalated.
    assert!(
        !procedures.contains("class WidgetCreateController extends _$WidgetCreateController {"),
        "the widgetCreate procedure's controller should not have kept the colliding class name:\n{procedures}"
    );
    assert!(
        procedures.contains("extends _$") && procedures.contains("WidgetCreateController"),
        "the widgetCreate procedure's controller should still exist under an escalated name:\n{procedures}"
    );
}

#[test]
fn riverpod_pubspec_adds_riverpod_annotation_generator_and_build_runner() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Riverpod);
    let pubspec = package_file(&package, "pubspec.yaml");

    assert!(
        pubspec.contains("flutter_riverpod: ^3.3.1"),
        "flutter_riverpod must stay exactly as the default preset already pins it:\n{pubspec}"
    );
    assert!(
        pubspec.contains("riverpod_annotation: 4.0.3"),
        "riverpod_annotation must be pinned to exactly 4.0.3 — riverpod_generator 4.0.4 (below) \
         itself depends on riverpod_annotation '4.0.3' as an exact pin, not a range:\n{pubspec}"
    );
    assert!(
        pubspec.contains("riverpod_generator: 4.0.4"),
        "riverpod_generator must be pinned to exactly 4.0.4 — the newest release still on \
         analyzer ^12.0.0, which resolves against Flutter stable's meta 1.18.0 pin on the real \
         SDK (Flutter 3.44.8/Dart 3.12.2), unlike newer riverpod_generator/build_runner \
         releases (verified by downloading that exact SDK and reproducing the failure for \
         real, not just reasoning from pub.dev version tables — see the pubspec.yaml.j2 \
         template's own comment for the full chain, including why a bare analyzer version pin \
         or a dependency_overrides resolves `pub get` but genuinely breaks `build_runner build` \
         at codegen time):\n{pubspec}"
    );
    assert!(
        pubspec.contains(r#"build_runner: ">=2.14.0 <2.15.0""#),
        "build_runner must stay capped below 2.15.0 to match the riverpod_generator 4.0.4 pin \
         above — 2.15.x tightened its own analyzer floor past what analyzer 12.x (what 4.0.4 \
         needs) satisfies:\n{pubspec}"
    );
    // A bare, non-Flutter `riverpod:` package must never be added
    // alongside `flutter_riverpod` — it already re-exports what
    // `@riverpod` needs.
    assert!(
        !pubspec.lines().any(|line| line.trim_start() == "riverpod:"),
        "a bare `riverpod:` dependency line should never be added:\n{pubspec}"
    );

    // Dependencies vs dev_dependencies placement: `riverpod_annotation`
    // must appear before the `dev_dependencies:` marker, and
    // `riverpod_generator`/`build_runner` after it.
    let dev_split = pubspec
        .find("dev_dependencies:")
        .expect("pubspec should have a dev_dependencies section");
    let annotation_index = pubspec
        .find("riverpod_annotation:")
        .expect("riverpod_annotation should be present");
    let generator_index = pubspec
        .find("riverpod_generator:")
        .expect("riverpod_generator should be present");
    let build_runner_index = pubspec
        .find("build_runner:")
        .expect("build_runner should be present");
    assert!(annotation_index < dev_split, "{pubspec}");
    assert!(generator_index > dev_split, "{pubspec}");
    assert!(build_runner_index > dev_split, "{pubspec}");
}

#[test]
fn default_preset_pubspec_stays_untouched_by_the_riverpod_only_additions() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Default);
    let pubspec = package_file(&package, "pubspec.yaml");

    assert!(!pubspec.contains("riverpod_annotation"), "{pubspec}");
    assert!(!pubspec.contains("riverpod_generator"), "{pubspec}");
    assert!(!pubspec.contains("build_runner"), "{pubspec}");
    assert!(pubspec.contains("flutter_riverpod: ^3.3.1"), "{pubspec}");
}

#[test]
fn override_proof_test_file_watches_the_existing_adapter_provider_and_the_new_list_provider() {
    let package = generate("tiny_rpc", "tiny_rpc_client", DartPreset::Riverpod);
    let test_file = package_file(&package, "test/tiny_rpc_client_test.dart");

    assert!(test_file.contains("tinyRpcClientAdapterProvider.overrideWithValue(fakeAdapter)"));
    assert!(test_file.contains("container.read(widgetListProvider.future)"));
    assert!(test_file.contains("class _FakeRpcAdapter implements CratestackRpcAdapter"));
}

// ---- Snapshot: the collision fixture's full generated output. ----

#[test]
fn riverpod_collision_snapshot_matches_fixture() {
    let package = generate(
        "riverpod_provider_collision",
        "dart_verify_riverpod_collision",
        DartPreset::Riverpod,
    );
    let snapshot_dir = snapshot_root().join("riverpod_provider_collision");
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

fn write_snapshot(dir: &Path, package: &GeneratedDartPackage) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("snapshot dir should be removable");
    }
    fs::create_dir_all(dir).expect("snapshot dir should be creatable");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("snapshot subdir should be creatable");
        }
        fs::write(&path, file.contents.as_bytes()).expect("snapshot file should write");
    }
}

fn assert_snapshot_matches(dir: &Path, package: &GeneratedDartPackage) {
    assert!(
        dir.exists(),
        "snapshot directory {dir:?} is missing — run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-dart` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing — run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {} — run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}
