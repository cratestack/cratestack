#![cfg(test)]
//! Integration tests for `--check` (drift-detection) mode on
//! `generate-typescript` and `generate-dart`.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::{handle_generate_dart, handle_generate_typescript};
use crate::cli_types::{DartPresetArg, TypeScriptPresetArg};

fn write_schema(dir: &TempDir, source: &str) -> PathBuf {
    let path = dir.path().join("schema.cstack");
    fs::write(&path, source).expect("write schema");
    path
}

const INITIAL_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
}
"#;

const EXTENDED_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
  note String?
}
"#;

fn generate_ts(schema: PathBuf, out: PathBuf, check: bool) -> anyhow::Result<()> {
    generate_ts_with_preset(schema, out, check, TypeScriptPresetArg::Default)
}

fn generate_ts_with_preset(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    preset: TypeScriptPresetArg,
) -> anyhow::Result<()> {
    handle_generate_typescript(
        schema,
        out,
        "cratestack-client".to_owned(),
        "/api".to_owned(),
        None,
        check,
        false,
        preset,
    )
}

fn generate_dart(schema: PathBuf, out: PathBuf, check: bool) -> anyhow::Result<()> {
    generate_dart_with_preset(schema, out, check, DartPresetArg::Default)
}

fn generate_dart_with_preset(
    schema: PathBuf,
    out: PathBuf,
    check: bool,
    preset: DartPresetArg,
) -> anyhow::Result<()> {
    handle_generate_dart(
        schema,
        out,
        "cratestack_client".to_owned(),
        "/api".to_owned(),
        None,
        check,
        preset,
    )
}

#[test]
fn typescript_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema.clone(), out.clone(), false).expect("initial generate");
    generate_ts(schema, out, true).expect("check should pass on unmodified output");
}

#[test]
fn typescript_check_fails_and_lists_files_after_schema_change() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema_path.clone(), out.clone(), false).expect("initial generate");

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

    let error =
        generate_ts(schema_path, out, true).expect_err("check should fail after schema change");
    assert!(error.to_string().contains("modified: src/models.ts"));
}

#[test]
fn typescript_check_flags_hand_edited_file_with_no_schema_change() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema.clone(), out.clone(), false).expect("initial generate");

    let models_path = out.join("src/models.ts");
    let original = fs::read_to_string(&models_path).unwrap();
    fs::write(&models_path, format!("{original}\n// hand-edited\n")).unwrap();

    let error = generate_ts(schema, out, true).expect_err("hand-edited file should be flagged");
    assert!(error.to_string().contains("modified: src/models.ts"));
}

#[test]
fn typescript_check_does_not_write_files() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts(schema_path.clone(), out.clone(), false).expect("initial generate");
    let before = fs::read_to_string(out.join("src/models.ts")).unwrap();

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();
    let _ = generate_ts(schema_path, out.clone(), true);

    let after = fs::read_to_string(out.join("src/models.ts")).unwrap();
    assert_eq!(
        before, after,
        "--check must not modify the output directory"
    );
}

// Issue #304: `--check` must be preset-aware — the expected file *set*
// differs between `default` and `swr` (`src/models.ts` vs.
// `src/models/<model>.ts` + `src/models/shared.ts` + `src/procedures.ts`,
// no `src/client.ts`/`src/react-query.ts`), and neither direction should
// be treated as spurious drift just because the file lists don't match
// each other.

#[test]
fn typescript_swr_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_preset(schema.clone(), out.clone(), false, TypeScriptPresetArg::Swr)
        .expect("initial swr generate");
    generate_ts_with_preset(schema, out, true, TypeScriptPresetArg::Swr)
        .expect("check --preset swr should pass against its own unmodified output");
}

#[test]
fn typescript_swr_check_flags_default_preset_output_as_real_drift() {
    // Generate with the default preset, then run `--check --preset swr`
    // against the same directory: this must fail with real `missing`
    // (swr's files) and `unexpected` (default's `src/models.ts`, etc.)
    // entries — not silently pass just because both runs are "generated
    // TypeScript output". A drift-check that swallowed the differing file
    // set here would defeat the point of `--check` for anyone switching
    // presets.
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_ts_with_preset(
        schema.clone(),
        out.clone(),
        false,
        TypeScriptPresetArg::Default,
    )
    .expect("initial default generate");

    let error = generate_ts_with_preset(schema, out, true, TypeScriptPresetArg::Swr)
        .expect_err("check --preset swr against default-preset output should report drift");
    let message = error.to_string();
    assert!(
        message.contains("missing: src/models/account.ts"),
        "swr's per-model file should be reported missing:\n{message}"
    );
    assert!(
        message.contains("unexpected: src/models.ts"),
        "default's monolithic models.ts should be reported unexpected:\n{message}"
    );
}

#[test]
fn dart_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_dart(schema.clone(), out.clone(), false).expect("initial generate");
    generate_dart(schema, out, true).expect("check should pass on unmodified output");
}

#[test]
fn dart_check_fails_after_schema_change() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_dart(schema_path.clone(), out.clone(), false).expect("initial generate");

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

    generate_dart(schema_path, out, true).expect_err("check should fail after schema change");
}

/// Issue #301, acceptance criterion: `--check` must be preset-aware — a
/// riverpod-generated directory checked against `--preset riverpod`
/// reports no drift (the two file *sets* genuinely differ, so a
/// preset-blind check would report spurious drift on every file).
#[test]
fn dart_riverpod_check_passes_when_output_matches_schema() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("client");

    generate_dart_with_preset(schema.clone(), out.clone(), false, DartPresetArg::Riverpod)
        .expect("initial riverpod generate");
    generate_dart_with_preset(schema, out, true, DartPresetArg::Riverpod)
        .expect("check should pass on unmodified riverpod output");
}

/// Same schema, generated once under each preset into sibling
/// directories: each preset's own `--check` against its own directory
/// must stay clean — the file sets differ by design, and drift
/// detection must not conflate them.
#[test]
fn dart_check_does_not_conflate_default_and_riverpod_file_sets() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let default_out = dir.path().join("default_client");
    let riverpod_out = dir.path().join("riverpod_client");

    generate_dart(schema.clone(), default_out.clone(), false).expect("default generate");
    generate_dart_with_preset(
        schema.clone(),
        riverpod_out.clone(),
        false,
        DartPresetArg::Riverpod,
    )
    .expect("riverpod generate");

    generate_dart(schema.clone(), default_out, true).expect("default check should stay clean");
    generate_dart_with_preset(schema, riverpod_out, true, DartPresetArg::Riverpod)
        .expect("riverpod check should stay clean");
}
