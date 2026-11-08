//! Shared schema file loader for the three top-level include macros.
//! Macro-argument grammars (`db = Postgres`, `decimal = ...`) live in
//! `schema_args.rs`, re-exported below — see that file's doc for why the
//! split.

use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use syn::LitStr;

pub(super) use super::schema_args::{SchemaPathArgs, ServerDb, ServerSchemaArgs};
use super::schema_sha::SchemaShaConsts;

/// One file of the schema, as the macros see it (design §10).
pub(super) struct IncludedFile {
    /// Relative to `CARGO_MANIFEST_DIR`, `/`-separated (`..` when the file is
    /// outside it).
    pub(super) manifest_relative: String,
    /// The canonical path the file was read from (it feeds `include_str!`).
    pub(super) absolute: PathBuf,
}

pub(super) struct SchemaLiteral {
    /// The macro literal (`SCHEMA_PATH`).
    pub(super) relative: String,
    /// `CARGO_MANIFEST_DIR` joined with the literal.
    pub(super) resolved: PathBuf,
    pub(super) schema: cratestack_core::Schema,
    pub(super) sha: SchemaShaConsts,
    /// Load order, root first.
    pub(super) files: Vec<IncludedFile>,
}

/// A `compile_error!` at the macro literal, as a `TokenStream`.
pub(super) fn compile_error(schema_path: &LitStr, message: impl std::fmt::Display) -> TokenStream {
    syn::Error::new(schema_path.span(), message.to_string())
        .to_compile_error()
        .into()
}

pub(super) fn parse_schema_literal(schema_path: &LitStr) -> Result<SchemaLiteral, TokenStream> {
    let relative = schema_path.value();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let resolved = PathBuf::from(&manifest_dir).join(&relative);
    let loaded =
        cratestack_parser::load_schema(&resolved, &cratestack_parser::LoadOptions::default())
            .map_err(|error| {
                TokenStream::from(
                    syn::Error::new(schema_path.span(), error.render()).to_compile_error(),
                )
            })?;
    let schema = loaded.schema.clone();

    reject_composite_primary_keys(schema_path, &schema)?;

    let sha = SchemaShaConsts::from_schema(&schema);
    let files = included_files(&manifest_dir, &loaded);

    Ok(SchemaLiteral {
        relative,
        resolved,
        schema,
        sha,
        files,
    })
}

/// Each file is included from the path that was actually read (its canonical
/// path), so a symlinked root, an absolute literal or a `..` import all name
/// files that exist. The label shown beside it is that path relative to the
/// manifest directory.
fn included_files(
    manifest_dir: &str,
    loaded: &cratestack_parser::LoadedSchema,
) -> Vec<IncludedFile> {
    let manifest =
        std::fs::canonicalize(manifest_dir).unwrap_or_else(|_| PathBuf::from(manifest_dir));
    loaded
        .files
        .iter()
        .map(|file| IncludedFile {
            manifest_relative: manifest_label(&manifest, &file.path),
            absolute: file.path.clone(),
        })
        .collect()
}

/// `file` as seen from `manifest`, `/`-separated, climbing with `..`.
fn manifest_label(manifest: &Path, file: &Path) -> String {
    let from: Vec<_> = manifest.components().collect();
    let to: Vec<_> = file.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        // Nothing in common (another drive): the path itself.
        return file.display().to_string().replace('\\', "/");
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// `@@id([...])` composite primary keys are parsed and validated by
/// `cratestack-parser`, and `cratestack-migrate` already emits correct
/// composite `PRIMARY KEY` DDL for them — but query builders, axum/RPC
/// routing, and all three client generators still assume exactly one
/// scalar PK column throughout (`ModelDescriptor<M, PK>` and friends).
/// Fail here with one clear message instead of letting a model with
/// `@@id(...)` reach codegen and panic somewhere deep in a `.find(...)
/// .expect(...)` call with no useful context.
///
/// Tracking: <https://github.com/cratestack/cratestack/issues/136>.
fn reject_composite_primary_keys(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    if let Some(model) = find_composite_id_model(schema) {
        return Err(TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                ::cratestack_core::composite_id::composite_id_unsupported_message(&model.name),
            )
            .to_compile_error(),
        ));
    }
    Ok(())
}

// Delegates to `cratestack_core::composite_id` so the CLI generators
// (which panicked on exactly these schemas until 2026-08-13, because
// they had no equivalent guard) match this path's behaviour by
// construction rather than by a second hand-copied `starts_with`.
fn find_composite_id_model(schema: &cratestack_core::Schema) -> Option<&cratestack_core::Model> {
    ::cratestack_core::composite_id::find_composite_id_model(schema)
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod included_files_tests;

#[cfg(test)]
mod tests {
    use super::find_composite_id_model;

    #[test]
    fn flags_model_with_composite_id_attribute() {
        let schema = cratestack_parser::parse_schema(
            r#"
model AccountMembership {
  accountId Int
  subject String

  @@id([accountId, subject])
}
"#,
        )
        .expect("schema should parse");

        let flagged = find_composite_id_model(&schema);
        assert_eq!(
            flagged.map(|model| model.name.as_str()),
            Some("AccountMembership")
        );
    }

    #[test]
    fn does_not_flag_single_field_id() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Account {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(find_composite_id_model(&schema).is_none());
    }
}
