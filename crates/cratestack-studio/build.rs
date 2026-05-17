//! Build script: hand the Studio UI source tree to `eject` as a single
//! tarball in `$OUT_DIR`.
//!
//! Two execution contexts:
//! - Local dev: the sibling crate at `../cratestack-studio-ui/` is on
//!   disk. Pack it on every relevant change.
//! - `cargo publish --verify` (and crates.io consumers): only
//!   `embedded-ui.tar.gz` shipped inside the published tarball is
//!   available. Copy it through to `$OUT_DIR`.
//!
//! The sibling layout sidesteps cargo's hardcoded "exclude any
//! subdirectory containing its own Cargo.toml" rule that would
//! otherwise drop the UI sources from `cargo package` — see
//! rust-lang/cargo#2828.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::Builder;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("ui.tar.gz");

    let sibling = manifest_dir
        .parent()
        .expect("manifest dir has parent")
        .join("cratestack-studio-ui");
    let bundled = manifest_dir.join("embedded-ui.tar.gz");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=embedded-ui.tar.gz");

    if sibling.is_dir() {
        bundle_from_source(&sibling, &out).expect("pack ui sources");
        emit_rerun_for_tree(&sibling);
    } else if bundled.is_file() {
        fs::copy(&bundled, &out).expect("copy bundled ui tarball");
    } else {
        panic!(
            "cratestack-studio: no UI source at {} and no bundled tarball at {}. \
             Run `just bundle-studio-ui` before publishing.",
            sibling.display(),
            bundled.display(),
        );
    }
}

fn bundle_from_source(src: &Path, out: &Path) -> io::Result<()> {
    let f = fs::File::create(out)?;
    let enc = GzEncoder::new(f, Compression::default());
    let mut builder = Builder::new(enc);
    append_dir(&mut builder, src, src)?;
    builder.into_inner()?.finish()?;
    Ok(())
}

fn append_dir<W: io::Write>(builder: &mut Builder<W>, root: &Path, dir: &Path) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap();
        if should_skip(rel) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            append_dir(builder, root, &path)?;
        } else {
            builder.append_path_with_name(&path, rel)?;
        }
    }
    Ok(())
}

fn should_skip(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    s.starts_with("target")
        || s == "Cargo.lock"
        || s == ".gitignore"
        || s.starts_with(".trunk")
        || s.starts_with("dist")
}

fn emit_rerun_for_tree(root: &Path) {
    let Ok(rd) = fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        if should_skip(rel) {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => emit_rerun_for_tree(&path),
            Ok(_) => println!("cargo:rerun-if-changed={}", path.display()),
            _ => {}
        }
    }
}
