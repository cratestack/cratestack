use std::path::{Path, PathBuf};

/// Every non-directory entry under `dir`, recursively.
///
/// Symlinks are **never followed**: `DirEntry::file_type` reports the link
/// itself, where `Path::is_dir` follows it (cratestack#1235). Generator
/// output contains no symlinks, so anything reached *through* one — a pnpm
/// workspace links each dependency into `node_modules/` — is not generated
/// output, and `git check-ignore` refuses such paths outright ("beyond a
/// symbolic link", exit 128). A symlink entry is itself returned, undescended:
/// when it is gitignored (as `node_modules/dep` is) the filter drops it, and a
/// stray unignored one is reported as `unexpected` like any other file the
/// generator did not produce.
pub(super) fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_dir {
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
