//! Build script: hand the Studio UI to runtime via two `OUT_DIR`
//! artifacts.
//!
//! - `ui.tar.gz` — gzipped source tree consumed by `studio eject
//!   --with-ui`. Sourced from the `cratestack-studio-ui` sibling
//!   during dev, or from `embedded-ui.tar.gz` in the published crate.
//! - `ui-dist/` — Trunk's release build of the Leptos app, served by
//!   the `embed-ui` feature via `rust-embed`. Sourced from the
//!   sibling's `dist/` during dev (if present), or extracted from
//!   `embedded-ui-dist.tar.gz` in the published crate. Always created
//!   (possibly empty) so `rust-embed` compiles either way.
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
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive, Builder};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=embedded-ui.tar.gz");
    println!("cargo:rerun-if-changed=embedded-ui-dist.tar.gz");

    let sibling = manifest_dir
        .parent()
        .expect("manifest dir has parent")
        .join("cratestack-studio-ui");

    materialize_source_tarball(&manifest_dir, &sibling, &out_dir.join("ui.tar.gz"));
    materialize_dist_dir(&manifest_dir, &sibling, &out_dir.join("ui-dist"));
}

fn materialize_source_tarball(manifest_dir: &Path, sibling: &Path, out: &Path) {
    let bundled = manifest_dir.join("embedded-ui.tar.gz");
    if sibling.is_dir() {
        bundle_from_source(sibling, out).expect("pack ui sources");
        emit_rerun_for_tree(sibling);
    } else if bundled.is_file() {
        fs::copy(&bundled, out).expect("copy bundled ui tarball");
    } else {
        panic!(
            "cratestack-studio: no UI source at {} and no bundled tarball at {}. \
             Run `just bundle-studio-ui` before publishing.",
            sibling.display(),
            bundled.display(),
        );
    }
}

fn materialize_dist_dir(manifest_dir: &Path, sibling: &Path, out: &Path) {
    let _ = fs::remove_dir_all(out);
    fs::create_dir_all(out).expect("create OUT_DIR/ui-dist");

    let sibling_dist = sibling.join("dist");
    let bundled_dist = manifest_dir.join("embedded-ui-dist.tar.gz");

    if sibling_dist.is_dir() {
        copy_tree(&sibling_dist, out).expect("copy sibling dist");
        emit_rerun_for_tree(&sibling_dist);
    } else if bundled_dist.is_file() {
        extract_tarball(&bundled_dist, out).expect("extract dist tarball");
    } else {
        println!(
            "cargo:warning=cratestack-studio: no Trunk dist available (looked at {} and {}). \
             The `embed-ui` feature will fall back to the placeholder page. \
             Run `just bundle-studio-ui` to ship a real admin UI.",
            sibling_dist.display(),
            bundled_dist.display(),
        );
    }
}

fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&to)?;
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn extract_tarball(archive: &Path, dst: &Path) -> io::Result<()> {
    let f = fs::File::open(archive)?;
    let mut ar = Archive::new(GzDecoder::new(f));
    ar.unpack(dst)
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
