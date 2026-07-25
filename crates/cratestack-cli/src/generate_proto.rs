//! `cratestack generate-proto` — ticket #169 Part D.
//!
//! Not built on `cli_support::GeneratedFile`/`drift::check_drift` (see
//! `crate::drift`): those model a directory of many independently
//! regenerated files, diffed file-by-file. This command always produces
//! exactly two files at two fixed, independently derived paths — and one
//! of them, `<schema>.pb.lock`, has stateful merge semantics (carry
//! forward existing numbers, tombstone removed ones) that a stateless
//! generate-and-diff helper doesn't model. Hence a small dedicated
//! handler instead.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cratestack_proto::{PbLock, build_lock, emit_proto, synthesize_messages};

use crate::cli_support::parse_schema_or_render;

#[cfg(test)]
mod tests;

pub(crate) fn handle_generate_proto(
    schema: PathBuf,
    out: PathBuf,
    package: Option<String>,
    check: bool,
) -> Result<()> {
    let lock_path = schema.with_extension("pb.lock");
    let existing_lock = read_existing_lock(&lock_path)?;
    let resolved_package = resolve_package(&lock_path, existing_lock.as_ref(), package)?;

    let parsed_schema = parse_schema_or_render(&schema)?;
    let extra_messages = synthesize_messages(&parsed_schema)
        .context("failed to synthesize Create/Update/procedure/Page message shapes")?;
    let mut new_lock = build_lock(&parsed_schema, existing_lock.as_ref(), &extra_messages)
        .context("failed to build the protobuf field-number lock")?;
    new_lock.package = Some(resolved_package);

    let proto_text = emit_proto(
        &parsed_schema,
        &new_lock,
        &extra_messages,
        &schema.display().to_string(),
    )
    .context("failed to emit .proto text")?;

    if check {
        return run_check(
            &lock_path,
            existing_lock.as_ref(),
            &new_lock,
            &out,
            &proto_text,
        );
    }

    std::fs::write(&lock_path, new_lock.to_toml())
        .with_context(|| format!("failed to write '{}'", lock_path.display()))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory '{}'", parent.display()))?;
    }
    std::fs::write(&out, proto_text)
        .with_context(|| format!("failed to write '{}'", out.display()))?;
    println!(
        "generated .proto: {} (lock: {})",
        out.display(),
        lock_path.display()
    );
    Ok(())
}

fn read_existing_lock(lock_path: &Path) -> Result<Option<PbLock>> {
    if !lock_path.exists() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(lock_path)
        .with_context(|| format!("failed to read '{}'", lock_path.display()))?;
    let lock = PbLock::from_toml(&source)
        .with_context(|| format!("failed to parse '{}'", lock_path.display()))?;
    Ok(Some(lock))
}

/// `docs/design/protobuf.md` §4.6: `--package` is required on first run
/// and locked thereafter, because the package name is part of the wire
/// identity (`/shop_api.Api/ModelUserList`).
fn resolve_package(
    lock_path: &Path,
    existing_lock: Option<&PbLock>,
    package: Option<String>,
) -> Result<String> {
    let locked = existing_lock.and_then(|lock| lock.package.clone());
    match (locked, package) {
        (Some(locked), Some(passed)) if locked != passed => bail!(
            "--package is `{passed}` but {} already pins `{locked}`; package is part of the wire \
             identity (it appears in every fully-qualified message/service name) and changing it \
             is a wire break — edit the lock by hand if you mean to do this deliberately",
            lock_path.display()
        ),
        (Some(locked), Some(_)) => Ok(locked),
        (Some(locked), None) => Ok(locked),
        (None, Some(passed)) => Ok(passed),
        (None, None) => bail!(
            "--package is required on first run (no existing {}); protobuf's package name is \
             part of the wire identity and cratestack-proto refuses to default it — see \
             docs/design/protobuf.md §4.6",
            lock_path.display()
        ),
    }
}

fn run_check(
    lock_path: &Path,
    existing_lock: Option<&PbLock>,
    new_lock: &PbLock,
    out: &Path,
    proto_text: &str,
) -> Result<()> {
    let lock_changed = existing_lock != Some(new_lock);
    let on_disk_proto = std::fs::read_to_string(out).ok();
    let proto_changed = on_disk_proto.as_deref() != Some(proto_text);

    if !lock_changed && !proto_changed {
        println!(
            "no drift: '{}' and '{}' match the schema",
            lock_path.display(),
            out.display()
        );
        return Ok(());
    }

    let mut report = String::from("drift detected:\n");
    if lock_changed {
        let status = if existing_lock.is_none() {
            "would be created"
        } else {
            "would change"
        };
        report.push_str(&format!("  {}: {status}\n", lock_path.display()));
    }
    if proto_changed {
        let status = if on_disk_proto.is_none() {
            "would be created"
        } else {
            "would change"
        };
        report.push_str(&format!("  {}: {status}\n", out.display()));
    }
    bail!(report.trim_end().to_owned());
}
