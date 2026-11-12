use std::collections::HashSet;
use std::path::Path;

use anyhow::{Result, bail};

use crate::cli_support::GeneratedFile;

mod git_ignore;
mod walk;

use git_ignore::git_ignored;
use walk::walk_files;

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
mod tests;
#[cfg(test)]
mod tests_git;
#[cfg(test)]
mod tests_symlink;
