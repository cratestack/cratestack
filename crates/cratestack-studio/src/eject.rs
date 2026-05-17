//! `cratestack studio eject` — produce a customizable starter project
//! that embeds CrateStack Studio against your own `.cstack` schemas.
//!
//! The default eject hands the caller a self-contained Cargo binary
//! crate:
//!
//! ```text
//! <out>/
//! ├── Cargo.toml
//! ├── README.md
//! ├── studio.toml
//! ├── schemas/example.cstack
//! └── src/main.rs
//! ```
//!
//! Pair with `--with-ui` to also drop the full Leptos+Trunk UI sources
//! into `<out>/ui/` for in-place customization. The UI source tree is
//! shipped inside the studio crate as a gzipped tarball produced by
//! `build.rs`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

const UI_TARBALL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.tar.gz"));

const STARTER_CARGO_TOML: &str = include_str!("../templates/starter/Cargo.toml.template");
const STARTER_README: &str = include_str!("../templates/starter/README.md.template");
const STARTER_MAIN: &str = include_str!("../templates/starter/main.rs");
const STARTER_STUDIO_TOML: &str = include_str!("../templates/starter/studio.toml");
const STARTER_EXAMPLE_CSTACK: &str = include_str!("../templates/starter/example.cstack");

#[derive(Debug, Clone)]
pub struct EjectOptions {
    pub out: PathBuf,
    /// Project name written into `Cargo.toml` / `README.md`. Defaults
    /// to the output directory's file name when unset.
    pub name: Option<String>,
    /// When `true`, an existing non-empty output directory is
    /// overwritten file-by-file. When `false` (default), eject
    /// refuses to write into a non-empty directory.
    pub force: bool,
    /// When `true`, also unpack the Leptos UI sources into
    /// `<out>/ui/` so callers can customize the front-end.
    pub with_ui: bool,
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
    #[error("failed to unpack embedded UI tarball: {0}")]
    Unpack(#[source] io::Error),
}

#[derive(Debug, Clone)]
pub struct EjectReport {
    pub out: PathBuf,
    pub written: Vec<PathBuf>,
    pub with_ui: bool,
}

pub fn eject(options: &EjectOptions) -> Result<EjectReport, EjectError> {
    ensure_writable_target(&options.out, options.force)?;
    fs::create_dir_all(&options.out).map_err(|source| EjectError::CreateDir {
        path: options.out.clone(),
        source,
    })?;

    let name = derive_name(options);
    let cargo_version = env!("CARGO_PKG_VERSION");
    let mut written = Vec::<PathBuf>::new();

    let cargo_toml = render_template(
        STARTER_CARGO_TOML,
        &[("name", &name), ("cratestack_version", cargo_version)],
    );
    write_file(
        &options.out,
        "Cargo.toml",
        cargo_toml.as_bytes(),
        &mut written,
    )?;
    let readme = render_template(STARTER_README, &[("name", &name)]);
    write_file(&options.out, "README.md", readme.as_bytes(), &mut written)?;
    write_file(
        &options.out,
        "studio.toml",
        STARTER_STUDIO_TOML.as_bytes(),
        &mut written,
    )?;
    write_file(
        &options.out,
        "schemas/example.cstack",
        STARTER_EXAMPLE_CSTACK.as_bytes(),
        &mut written,
    )?;
    write_file(
        &options.out,
        "src/main.rs",
        STARTER_MAIN.as_bytes(),
        &mut written,
    )?;

    if options.with_ui {
        unpack_ui_sources(&options.out.join("ui"), &mut written)?;
    }

    Ok(EjectReport {
        out: options.out.clone(),
        written,
        with_ui: options.with_ui,
    })
}

fn derive_name(options: &EjectOptions) -> String {
    if let Some(name) = options.name.as_deref().filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    options
        .out
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("cratestack-studio-app")
        .to_string()
}

fn render_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

fn write_file(
    root: &Path,
    rel: &str,
    bytes: &[u8],
    written: &mut Vec<PathBuf>,
) -> Result<(), EjectError> {
    let dest = root.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|source| EjectError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&dest, bytes).map_err(|source| EjectError::Write {
        path: dest.clone(),
        source,
    })?;
    written.push(PathBuf::from(rel));
    Ok(())
}

fn ensure_writable_target(out: &Path, force: bool) -> Result<(), EjectError> {
    if !out.exists() || force {
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

fn unpack_ui_sources(root: &Path, written: &mut Vec<PathBuf>) -> Result<(), EjectError> {
    fs::create_dir_all(root).map_err(|source| EjectError::CreateDir {
        path: root.to_path_buf(),
        source,
    })?;
    let mut archive = Archive::new(GzDecoder::new(UI_TARBALL));
    for entry in archive.entries().map_err(EjectError::Unpack)? {
        let mut entry = entry.map_err(EjectError::Unpack)?;
        let rel = entry.path().map_err(EjectError::Unpack)?.into_owned();
        let dest = root.join(&rel);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&dest)
                .map_err(|source| EjectError::CreateDir { path: dest, source })?;
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
        written.push(PathBuf::from("ui").join(&rel));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
