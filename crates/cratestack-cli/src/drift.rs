use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};

use crate::cli_support::GeneratedFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriftKind {
    /// Generated content differs from what's on disk.
    Modified,
    /// The generator would produce this file but it's absent on disk.
    Missing,
    /// Present on disk but the generator no longer produces it.
    Unexpected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriftEntry {
    pub(crate) file_name: String,
    pub(crate) kind: DriftKind,
}

/// Diffs freshly generated `files` against whatever already exists under
/// `out`, file by file, without writing anything.
pub(crate) fn diff_generated_files(out: &Path, files: &[GeneratedFile]) -> Vec<DriftEntry> {
    let mut drift = Vec::new();
    let mut expected = HashSet::new();

    for file in files {
        let destination = out.join(&file.file_name);
        expected.insert(destination.clone());
        match std::fs::read_to_string(&destination) {
            Ok(existing) if existing == file.contents => {}
            Ok(_) => drift.push(DriftEntry {
                file_name: file.file_name.clone(),
                kind: DriftKind::Modified,
            }),
            Err(_) => drift.push(DriftEntry {
                file_name: file.file_name.clone(),
                kind: DriftKind::Missing,
            }),
        }
    }

    let on_disk = walk_files(out);
    let ignored = git_ignored(out, &on_disk);
    for path in on_disk {
        if !expected.contains(&path) && !ignored.contains(&path) {
            let relative = path
                .strip_prefix(out)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            drift.push(DriftEntry {
                file_name: relative,
                kind: DriftKind::Unexpected,
            });
        }
    }

    drift.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    drift
}

/// Which of `candidates` git considers ignored, so they can be excluded
/// from the `Unexpected` arm.
///
/// `--check` exists to keep *committed* generated output in sync with the
/// schema, and a file that isn't committed cannot drift from it. Without
/// this, running the check after `flutter pub get` / `build_runner` reports
/// every `.dart_tool/` entry, `*.g.dart`, `*.mapper.dart` and `pubspec.lock`
/// as drift while the generated content matches perfectly (cratestack#659) —
/// and the workaround for that false failure is indistinguishable from the
/// workaround for a real one.
///
/// Delegating to `git check-ignore` rather than reimplementing gitignore
/// matching (anchoring, negation, `**`, nested `.gitignore` files) or taking
/// on the `ignore` crate's `globset`/`regex-automata` subtree: git is
/// necessarily present for a checkout that has a `.gitignore` to honour in
/// the first place.
///
/// Returns empty — i.e. preserves the previous behaviour exactly — when
/// `out` is not inside a work tree, when git is unavailable, or when the
/// call fails. It also returns empty when `out` is *itself* ignored (a
/// scratch directory under `target/`, say): everything beneath such a
/// directory is ignored, so filtering on that basis would silently disable
/// stale-file detection altogether rather than refine it.
fn git_ignored(out: &Path, candidates: &[PathBuf]) -> HashSet<PathBuf> {
    let empty = HashSet::new();
    if candidates.is_empty() || !is_tracked_territory(out) {
        return empty;
    }

    // Feed ABSOLUTE paths. `git -C <out>` resolves relative arguments
    // against `out`, not against our CWD — so a relative `--out` (which is
    // what `just regen-examples` passes) turned every candidate into a
    // nonexistent `<out>/<out>/...`. Basename rules like `*.g.dart` still
    // matched, path-anchored ones like `client/.dart_tool/` did not, and the
    // filter silently did about a third of its job. `absolute` rather than
    // `canonicalize` so this does not depend on the path existing or resolve
    // symlinks out from under the caller's own spelling of it.
    let mut absolute = Vec::with_capacity(candidates.len());
    let mut stdin = Vec::new();
    for path in candidates {
        let Ok(abs) = std::path::absolute(path) else {
            continue;
        };
        stdin.extend_from_slice(abs.as_os_str().as_encoded_bytes());
        stdin.push(0);
        absolute.push((abs, path));
    }
    if stdin.is_empty() {
        return empty;
    }

    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return empty;
    };
    if let Some(pipe) = child.stdin.as_mut() {
        // A broken pipe here just means git bailed early; the empty/partial
        // stdout below is then handled like any other inconclusive result.
        let _ = pipe.write_all(&stdin);
    }
    drop(child.stdin.take());
    let Ok(output) = child.wait_with_output() else {
        return empty;
    };
    // Exit 0 = some paths ignored, 1 = none ignored (not an error),
    // 128 = fatal (not a repository). Only 0 carries a path list.
    if output.status.code() != Some(0) {
        return empty;
    }

    // git echoes back exactly the strings it was given, so map the absolute
    // paths in its answer back to the caller's own spelling.
    let reported = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(String::from_utf8_lossy(line).into_owned()))
        .collect::<HashSet<_>>();

    absolute
        .into_iter()
        .filter(|(abs, _)| reported.contains(abs))
        .map(|(_, original)| original.clone())
        .collect()
}

/// Whether `out` sits in a git work tree *and* is not itself ignored.
fn is_tracked_territory(out: &Path) -> bool {
    let inside = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(inside, Ok(status) if status.success()) {
        return false;
    }
    // `check-ignore -q` exits 0 when the path IS ignored.
    let self_ignored = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["check-ignore", "-q", "."])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    !matches!(self_ignored, Ok(status) if status.success())
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // git treats `.git` as *excluded*, not ignored, so
            // `check-ignore` never reports its contents — it has to be
            // skipped here or a `--out` that happens to be a repository
            // root reports its own object store as drift.
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            files.extend(walk_files(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// Runs the `--check` (drift-detection) mode for a generated file set
/// (a client package, or — for `generate-wiremock` — a set of stub
/// mappings): diffs in-memory `files` against `out` and, if they
/// differ, fails with a report instead of writing anything to disk.
pub(crate) fn check_drift(out: &Path, files: &[GeneratedFile], label: &str) -> Result<()> {
    let drift = diff_generated_files(out, files);
    if drift.is_empty() {
        println!(
            "no drift detected: generated {label} output matches '{}'",
            out.display()
        );
        return Ok(());
    }

    let mut report = format!(
        "drift detected in '{}': {} file(s) differ from the generated {label} output\n",
        out.display(),
        drift.len()
    );
    for entry in &drift {
        let kind_label = match entry.kind {
            DriftKind::Modified => "modified",
            DriftKind::Missing => "missing",
            DriftKind::Unexpected => "unexpected",
        };
        report.push_str(&format!("  {kind_label}: {}\n", entry.file_name));
    }
    bail!(report.trim_end().to_owned());
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn file(name: &str, contents: &str) -> GeneratedFile {
        GeneratedFile {
            file_name: name.to_owned(),
            contents: contents.to_owned(),
        }
    }

    #[test]
    fn no_drift_when_disk_matches_generated_output() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("index.ts"), "export const x = 1;\n").unwrap();

        let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
        assert!(drift.is_empty());
    }

    #[test]
    fn flags_hand_edited_file_as_modified() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("index.ts"), "export const x = 999;\n").unwrap();

        let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
        assert_eq!(
            drift,
            vec![DriftEntry {
                file_name: "index.ts".to_owned(),
                kind: DriftKind::Modified,
            }]
        );
    }

    #[test]
    fn flags_new_generated_file_as_missing() {
        let dir = TempDir::new().expect("tempdir");

        let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
        assert_eq!(
            drift,
            vec![DriftEntry {
                file_name: "index.ts".to_owned(),
                kind: DriftKind::Missing,
            }]
        );
    }

    #[test]
    fn flags_stale_disk_file_as_unexpected() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("index.ts"), "export const x = 1;\n").unwrap();
        std::fs::write(dir.path().join("stale.ts"), "export const y = 2;\n").unwrap();

        let drift = diff_generated_files(dir.path(), &[file("index.ts", "export const x = 1;\n")]);
        assert_eq!(
            drift,
            vec![DriftEntry {
                file_name: "stale.ts".to_owned(),
                kind: DriftKind::Unexpected,
            }]
        );
    }

    /// `git init` a directory and drop a `.gitignore` in it. No commit is
    /// needed — `check-ignore` reads the working tree's ignore rules, not
    /// history.
    fn init_repo(dir: &Path, gitignore: &str) {
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

    #[test]
    fn check_drift_ok_when_clean() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("index.ts"), "export const x = 1;\n").unwrap();

        check_drift(
            dir.path(),
            &[file("index.ts", "export const x = 1;\n")],
            "TypeScript",
        )
        .expect("no drift");
    }

    #[test]
    fn check_drift_errors_and_lists_files_when_dirty() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("index.ts"), "export const x = 999;\n").unwrap();

        let error = check_drift(
            dir.path(),
            &[file("index.ts", "export const x = 1;\n")],
            "TypeScript",
        )
        .expect_err("drift should fail check");
        assert!(error.to_string().contains("modified: index.ts"));
    }

    #[test]
    fn check_drift_leaves_disk_untouched() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("index.ts"), "export const x = 999;\n").unwrap();

        let _ = check_drift(
            dir.path(),
            &[file("index.ts", "export const x = 1;\n")],
            "TypeScript",
        );

        assert_eq!(
            std::fs::read_to_string(dir.path().join("index.ts")).unwrap(),
            "export const x = 999;\n"
        );
    }
}
