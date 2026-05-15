//! `cratestack studio eject` — copy the Leptos+Trunk UI sources into a
//! writable directory.
//!
//! The UI tree at `crates/cratestack-studio/ui/` is embedded at compile
//! time via `include_dir!`. At runtime we walk the tree and write each
//! file out to the caller's target directory, then prepend a small
//! README that points at the upstream and explains how the standalone
//! workflow lines up against the in-tree one.
//!
//! Generated artifacts (`/dist`, `/target`, `Cargo.lock`) are
//! intentionally skipped — they'd bloat the eject output and the
//! consumer can regenerate them on first `trunk build`.

use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

/// Embedded UI source tree. Refreshed every time
/// `cratestack-studio` is rebuilt — the eject output is therefore
/// tied to the framework version that built the binary.
const UI_TREE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui");

/// Bytes written into the ejected directory's `README.md`, replacing
/// whatever ships in the in-tree UI crate (which is empty in Phase
/// 1b). Self-contained so callers know how to build + serve without
/// flipping back to the framework docs.
const EJECT_README: &str = include_str!("../templates/eject/README.md");

#[derive(Debug, Clone)]
pub struct EjectOptions {
    pub out: PathBuf,
    /// When `true`, an existing non-empty output directory is
    /// overwritten file-by-file. When `false` (default), eject
    /// refuses to write into a non-empty directory.
    pub force: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum EjectError {
    #[error("output directory '{path}' already exists and is not empty; pass --force to overwrite")]
    OutputNotEmpty { path: PathBuf },
    #[error("failed to create '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write '{path}': {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to inspect '{path}': {source}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Files / directories in the embedded UI tree that we don't want to
/// hand to the user.
fn should_skip(rel_path: &Path) -> bool {
    let path_str = rel_path.to_string_lossy();
    path_str.starts_with("dist/")
        || path_str == "dist"
        || path_str.starts_with("target/")
        || path_str == "target"
        || path_str == "Cargo.lock"
        || path_str.starts_with(".trunk/")
}

/// Render and write the UI tree into `options.out`. Existing files
/// with the same path are overwritten; the README in the ejected dir
/// is replaced with [`EJECT_README`] regardless of what ships in the
/// embedded tree.
pub fn eject(options: &EjectOptions) -> Result<EjectReport, EjectError> {
    ensure_writable_target(&options.out, options.force)?;
    fs::create_dir_all(&options.out).map_err(|source| EjectError::CreateDir {
        path: options.out.clone(),
        source,
    })?;

    let mut written = Vec::<PathBuf>::new();
    write_dir(&UI_TREE, &options.out, &mut written)?;

    let readme_path = options.out.join("README.md");
    fs::write(&readme_path, EJECT_README).map_err(|source| EjectError::Write {
        path: readme_path.clone(),
        source,
    })?;
    if !written.iter().any(|p| p.ends_with("README.md")) {
        written.push(PathBuf::from("README.md"));
    }

    Ok(EjectReport {
        out: options.out.clone(),
        written,
    })
}

#[derive(Debug, Clone)]
pub struct EjectReport {
    pub out: PathBuf,
    pub written: Vec<PathBuf>,
}

fn ensure_writable_target(out: &Path, force: bool) -> Result<(), EjectError> {
    if !out.exists() {
        return Ok(());
    }
    if force {
        return Ok(());
    }
    let mut entries = fs::read_dir(out).map_err(|source| EjectError::Inspect {
        path: out.to_path_buf(),
        source,
    })?;
    if entries.next().is_some() {
        return Err(EjectError::OutputNotEmpty {
            path: out.to_path_buf(),
        });
    }
    Ok(())
}

fn write_dir(dir: &Dir<'_>, root: &Path, written: &mut Vec<PathBuf>) -> Result<(), EjectError> {
    for entry in dir.entries() {
        let rel = entry.path();
        if should_skip(rel) {
            continue;
        }
        match entry {
            include_dir::DirEntry::Dir(sub) => {
                let dest = root.join(rel);
                fs::create_dir_all(&dest).map_err(|source| EjectError::CreateDir {
                    path: dest.clone(),
                    source,
                })?;
                write_dir(sub, root, written)?;
            }
            include_dir::DirEntry::File(file) => {
                let dest = root.join(rel);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).map_err(|source| EjectError::CreateDir {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::write(&dest, file.contents()).map_err(|source| EjectError::Write {
                    path: dest.clone(),
                    source,
                })?;
                written.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
