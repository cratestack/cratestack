use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::tests::file;
use super::*;

/// `git init` a directory and drop a `.gitignore` in it. No commit is
/// needed — `check-ignore` reads the working tree's ignore rules, not
/// history.
pub(super) fn init_repo(dir: &Path, gitignore: &str) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["init", "--quiet"])
        .status()
        .expect("git must be available to run these tests");
    assert!(status.success(), "git init failed in {}", dir.display());
    std::fs::write(dir.join(".gitignore"), gitignore).unwrap();
}

/// cratestack#659: build output under `--out` is not drift. Before the
/// fix this reported four `unexpected` entries and exited non-zero while
/// the generated content matched perfectly.
#[test]
fn gitignored_build_output_is_not_drift() {
    let repo = TempDir::new().expect("tempdir");
    init_repo(
        repo.path(),
        ".dart_tool/\n*.g.dart\n*.mapper.dart\npubspec.lock\n",
    );
    // The generated client lives in a subdirectory of the repo, as
    // `examples/flutter-riverpod/client` does — not at the repo root.
    let out = repo.path().join("client");

    std::fs::create_dir_all(out.join("lib/src/models")).unwrap();
    std::fs::create_dir_all(out.join(".dart_tool")).unwrap();
    std::fs::write(out.join("lib/src/models/board.dart"), "class Board {}\n").unwrap();
    // Exactly the artifacts `flutter pub get` + `build_runner` leave.
    std::fs::write(out.join("lib/src/models/board.g.dart"), "// generated\n").unwrap();
    std::fs::write(
        out.join("lib/src/models/board.mapper.dart"),
        "// generated\n",
    )
    .unwrap();
    std::fs::write(out.join(".dart_tool/package_config.json"), "{}\n").unwrap();
    std::fs::write(out.join("pubspec.lock"), "# lock\n").unwrap();

    let drift = diff_generated_files(
        &out,
        &[file("lib/src/models/board.dart", "class Board {}\n")],
    );
    assert!(
        drift.is_empty(),
        "gitignored build output must not count as drift, got: {drift:?}"
    );
}

/// The other half of the same change: filtering must not swallow a
/// genuinely stale committed file. A model dropped from the schema
/// leaves its `.dart` behind, and that still has to be reported.
#[test]
fn stale_tracked_file_is_still_unexpected_in_a_git_repo() {
    let repo = TempDir::new().expect("tempdir");
    init_repo(repo.path(), ".dart_tool/\n*.g.dart\n");
    let out = repo.path().join("client");

    std::fs::create_dir_all(out.join("lib/src/models")).unwrap();
    std::fs::write(out.join("lib/src/models/board.dart"), "class Board {}\n").unwrap();
    // Not ignored — a real leftover from a model that left the schema.
    std::fs::write(
        out.join("lib/src/models/removed.dart"),
        "class Removed {}\n",
    )
    .unwrap();
    // ...alongside build output, to prove one is filtered and the other isn't.
    std::fs::write(out.join("lib/src/models/board.g.dart"), "// generated\n").unwrap();

    let drift = diff_generated_files(
        &out,
        &[file("lib/src/models/board.dart", "class Board {}\n")],
    );
    assert_eq!(
        drift,
        vec![DriftEntry {
            file_name: "lib/src/models/removed.dart".to_owned(),
            kind: DriftKind::Unexpected,
        }],
        "a tracked file the generator no longer emits must still be flagged"
    );
}

/// `just regen-examples` passes `--out` as a path relative to the repo
/// root, and the first cut of the gitignore filter fed those relative
/// paths straight to `git -C <out> check-ignore`, which resolves them
/// against `out` rather than the CWD. Basename rules (`*.g.dart`) still
/// matched; path-anchored ones (`client/.dart_tool/`) silently did not,
/// so the filter dropped 7 of 24 ignored files and the real command still
/// failed. Every other test here uses an absolute `TempDir` path and so
/// could not see it.
///
/// Uses a `../`-relative path rather than `set_current_dir`: the CWD is
/// process-global and these tests run on parallel threads, so mutating it
/// would make every other filesystem-touching test in the crate racy.
#[test]
fn relative_out_path_still_matches_path_anchored_ignore_rules() {
    let repo = TempDir::new().expect("tempdir");
    // Path-anchored on purpose: a bare `.dart_tool/` would match by
    // basename anywhere and would pass even with the bug present.
    init_repo(repo.path(), "client/.dart_tool/\n");
    let out = repo.path().join("client");
    std::fs::create_dir_all(out.join(".dart_tool/build")).unwrap();
    std::fs::write(out.join("index.dart"), "class A {}\n").unwrap();
    std::fs::write(out.join(".dart_tool/build/asset_graph.json"), "{}\n").unwrap();

    let drift = diff_generated_files(
        &relative_to_cwd(&out),
        &[file("index.dart", "class A {}\n")],
    );
    assert!(
        drift.is_empty(),
        "a path-anchored ignore rule must still apply when --out is relative, got: {drift:?}"
    );
}

/// `target` expressed relative to the process CWD — `../../..`-style,
/// since the temp dir and the CWD share only the filesystem root.
fn relative_to_cwd(target: &Path) -> PathBuf {
    let cwd = std::env::current_dir().expect("cwd");
    let mut relative = PathBuf::new();
    for _ in cwd.components().skip(1) {
        relative.push("..");
    }
    for component in target.components().skip(1) {
        relative.push(component);
    }
    assert!(
        relative.is_relative(),
        "helper must produce a relative path"
    );
    relative
}

/// When the output directory is itself ignored, every file under it is
/// ignored — filtering on that basis would disable stale-file detection
/// rather than refine it, so the check falls back to its old behaviour.
#[test]
fn fully_ignored_output_dir_keeps_reporting_unexpected_files() {
    let dir = TempDir::new().expect("tempdir");
    init_repo(dir.path(), "scratch/\n");
    let out = dir.path().join("scratch");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("index.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(out.join("stale.ts"), "export const y = 2;\n").unwrap();

    let drift = diff_generated_files(&out, &[file("index.ts", "export const x = 1;\n")]);
    assert_eq!(
        drift,
        vec![DriftEntry {
            file_name: "stale.ts".to_owned(),
            kind: DriftKind::Unexpected,
        }]
    );
}
