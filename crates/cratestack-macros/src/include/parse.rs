//! Shared schema file loader for the three top-level include macros.
//! Macro-argument grammars (`db = Postgres`, `decimal = ...`) live in
//! `schema_args.rs`, re-exported below — see that file's doc for why the
//! split.

use std::path::PathBuf;

use proc_macro::TokenStream;
use syn::LitStr;

pub(super) use super::schema_args::{SchemaPathArgs, ServerDb, ServerSchemaArgs};
use super::schema_sha::SchemaShaConsts;

/// One file of the schema, as the macros see it (design §10).
pub(super) struct IncludedFile {
    /// Relative to `CARGO_MANIFEST_DIR`, `/`-separated, lexically folded.
    pub(super) manifest_relative: String,
    /// `CARGO_MANIFEST_DIR` joined with the same, lexically folded and
    /// deliberately not canonicalized (it feeds `include_str!`).
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
    let files = included_files(&relative, &manifest_dir, &loaded);

    Ok(SchemaLiteral {
        relative,
        resolved,
        schema,
        sha,
        files,
    })
}

fn included_files(
    literal: &str,
    manifest_dir: &str,
    loaded: &cratestack_parser::LoadedSchema,
) -> Vec<IncludedFile> {
    let literal_dir = std::path::Path::new(literal)
        .parent()
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    loaded
        .files
        .iter()
        .map(|file| {
            let joined = format!("{literal_dir}/{}", file.relative);
            let manifest_relative = fold_lexically(&joined);
            let absolute = PathBuf::from(manifest_dir).join(&manifest_relative);
            IncludedFile {
                manifest_relative,
                absolute,
            }
        })
        .collect()
}

/// Fold `.` and `a/..` segments without touching the filesystem.
fn fold_lexically(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." if out.last().is_some_and(|last| *last != "..") => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.join("/")
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
mod tests {
    use super::{find_composite_id_model, fold_lexically};

    #[test]
    fn fold_lexically_resolves_dot_and_dotdot() {
        assert_eq!(
            fold_lexically("tests/fixtures/a/../shared/./c.cstack"),
            "tests/fixtures/shared/c.cstack"
        );
        assert_eq!(fold_lexically("/schema.cstack"), "schema.cstack");
        assert_eq!(fold_lexically("../x/y.cstack"), "../x/y.cstack");
    }

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
