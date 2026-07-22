use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

    for path in walk_files(out) {
        if !expected.contains(&path) {
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

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_files(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// Runs the `--check` (drift-detection) mode for a generated client
/// package: diffs in-memory `files` against `out` and, if they differ,
/// fails with a report instead of writing anything to disk.
pub(crate) fn check_drift(out: &Path, files: &[GeneratedFile], label: &str) -> Result<()> {
    let drift = diff_generated_files(out, files);
    if drift.is_empty() {
        println!(
            "no drift detected: generated {label} client package matches '{}'",
            out.display()
        );
        return Ok(());
    }

    let mut report = format!(
        "drift detected in '{}': {} file(s) differ from the generated {label} client package\n",
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
