use tempfile::TempDir;

use super::tests::file;
use super::tests_git::init_repo;
use super::*;

/// cratestack#1235 step 3: a pnpm-style symlinked directory inside a
/// gitignored `node_modules/` must neither be descended into nor stop the
/// plain gitignored file beside it from being filtered.
#[cfg(unix)]
#[test]
fn symlinked_dir_in_ignored_tree_is_not_drift() {
    let repo = TempDir::new().expect("tempdir");
    init_repo(repo.path(), "node_modules/\n");
    let outside = TempDir::new().expect("tempdir");
    std::fs::write(outside.path().join("package.json"), "{}\n").unwrap();
    let out = repo.path().join("pkg");
    std::fs::create_dir_all(out.join("node_modules")).unwrap();
    std::fs::write(out.join("index.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(out.join("node_modules/plain.txt"), "x\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), out.join("node_modules/dep")).unwrap();

    let drift = diff_generated_files(&out, &[file("index.ts", "export const x = 1;\n")]);
    assert!(
        drift.is_empty(),
        "ignored files and a symlinked dir under node_modules/ are not drift, got: {drift:?}"
    );
}

/// The symlink is listed but not followed, so a stray *unignored* one is
/// reported as itself and nothing beyond it is.
#[cfg(unix)]
#[test]
fn unignored_symlinked_dir_is_reported_once_without_descending() {
    let repo = TempDir::new().expect("tempdir");
    init_repo(repo.path(), "");
    let outside = TempDir::new().expect("tempdir");
    std::fs::write(outside.path().join("package.json"), "{}\n").unwrap();
    let out = repo.path().join("pkg");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("index.ts"), "export const x = 1;\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), out.join("link")).unwrap();

    let drift = diff_generated_files(&out, &[file("index.ts", "export const x = 1;\n")]);
    assert_eq!(
        drift,
        vec![DriftEntry {
            file_name: "link".to_owned(),
            kind: DriftKind::Unexpected,
        }]
    );
}

/// A batch `check-ignore` that dies with 128 must fall back to per-path
/// answers. Candidates beyond a symlink are fed to `git_ignored` directly
/// (the walk no longer produces them) next to a normally ignorable file.
#[cfg(unix)]
#[test]
fn git_exit_128_falls_back_to_per_path_checks() {
    let repo = TempDir::new().expect("tempdir");
    init_repo(repo.path(), "node_modules/\n");
    let outside = TempDir::new().expect("tempdir");
    std::fs::write(outside.path().join("package.json"), "{}\n").unwrap();
    let out = repo.path().join("pkg");
    std::fs::create_dir_all(out.join("node_modules")).unwrap();
    std::fs::write(out.join("node_modules/plain.txt"), "x\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), out.join("node_modules/dep")).unwrap();

    let plain = out.join("node_modules/plain.txt");
    let beyond = out.join("node_modules/dep/package.json");
    let ignored = super::git_ignore::git_ignored(&out, &[plain.clone(), beyond.clone()]);
    assert!(
        ignored.contains(&plain),
        "plain ignored file must survive the 128"
    );
    assert!(
        !ignored.contains(&beyond),
        "unanswerable path is not hidden"
    );
}

/// Outside any work tree nothing is ignored and nothing is filtered.
#[test]
fn outside_a_git_repo_nothing_is_ignored() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("index.ts"), "x\n").unwrap();
    std::fs::write(dir.path().join("stale.g.dart"), "x\n").unwrap();
    let drift = diff_generated_files(dir.path(), &[file("index.ts", "x\n")]);
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].file_name, "stale.g.dart");
}
