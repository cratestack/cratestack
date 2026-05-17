//! `cratestack studio eject` — copy the Leptos+Trunk UI sources into a
//! writable directory.
//!
//! The UI tree at `crates/cratestack-studio-ui/` is packed by
//! `build.rs` into a gzipped tarball in `OUT_DIR` and embedded via
//! `include_bytes!`. At runtime we stream the archive into the
//! caller's target directory, then write a small README that points
//! at the upstream and explains how the standalone workflow lines up
//! against the in-tree one.
//!
//! Generated artifacts (`/dist`, `/target`, `Cargo.lock`,
//! `.trunk/`) are filtered at pack time, so the runtime walk only
//! sees the source files we want to hand over.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

/// Gzipped tarball of the UI source tree, produced by `build.rs`.
/// Refreshed every time `cratestack-studio` is rebuilt — the eject
/// output is therefore tied to the framework version that built the
/// binary.
const UI_TARBALL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.tar.gz"));

/// Bytes written into the ejected directory's `README.md`, replacing
/// whatever ships in the in-tree UI crate. Self-contained so callers
/// know how to build + serve without flipping back to the framework
/// docs.
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
        source: io::Error,
    },
    #[error("failed to write '{path}': {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect '{path}': {source}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read embedded UI tarball: {0}")]
    Unpack(#[source] io::Error),
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

    let written = unpack_tarball(&options.out)?;

    let readme_path = options.out.join("README.md");
    fs::write(&readme_path, EJECT_README).map_err(|source| EjectError::Write {
        path: readme_path.clone(),
        source,
    })?;
    let mut written = written;
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

fn unpack_tarball(root: &Path) -> Result<Vec<PathBuf>, EjectError> {
    let mut archive = Archive::new(GzDecoder::new(UI_TARBALL));
    let mut written = Vec::<PathBuf>::new();
    let entries = archive.entries().map_err(EjectError::Unpack)?;
    for entry in entries {
        let mut entry = entry.map_err(EjectError::Unpack)?;
        let rel = entry
            .path()
            .map_err(EjectError::Unpack)?
            .into_owned();
        let dest = root.join(&rel);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&dest).map_err(|source| EjectError::CreateDir {
                path: dest,
                source,
            })?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|source| EjectError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        entry.unpack(&dest).map_err(|source| EjectError::Write {
            path: dest.clone(),
            source,
        })?;
        written.push(rel);
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
