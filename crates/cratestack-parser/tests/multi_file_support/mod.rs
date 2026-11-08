//! Shared helpers of the multi-file suites: schemas laid over fake absolute
//! paths with `OverlaySources`, so nothing touches the disk.
#![allow(dead_code)]

use cratestack_parser::{
    LoadOptions, LoadedSchema, OverlaySources, SchemaError, load_schema, load_schema_diagnostics,
};

pub const ROOT: &str = "/fixture/schema.cstack";

pub fn overlay(files: &[(&str, &str)]) -> OverlaySources {
    let mut sources = OverlaySources::new(false);
    for (path, text) in files {
        sources.insert(path, *text);
    }
    sources
}

pub fn load_from(
    files: &[(&str, &str)],
    root: &str,
    follow_part_of: bool,
) -> Result<LoadedSchema, SchemaError> {
    let sources = overlay(files);
    let options = LoadOptions {
        follow_part_of,
        sources: Some(&sources),
    };
    load_schema(root, &options)
}

pub fn load(files: &[(&str, &str)]) -> Result<LoadedSchema, SchemaError> {
    load_from(files, ROOT, false)
}

pub fn load_all_errors(files: &[(&str, &str)]) -> Vec<SchemaError> {
    let sources = overlay(files);
    let options = LoadOptions {
        follow_part_of: false,
        sources: Some(&sources),
    };
    load_schema_diagnostics(ROOT, &options).errors
}

fn text_of<'a>(files: &'a [(&str, &str)], file: &str) -> &'a str {
    files
        .iter()
        .find(|(path, _)| *path == file)
        .unwrap_or_else(|| panic!("no fixture file {file}"))
        .1
}

/// The text of `error`'s primary span, in the file it names.
pub fn primary_text<'a>(files: &'a [(&str, &str)], error: &SchemaError) -> &'a str {
    &text_of(files, error.file())[error.span()]
}

/// Asserts message, file and the text under the primary span.
pub fn assert_primary(
    files: &[(&str, &str)],
    error: &SchemaError,
    message: &str,
    file: &str,
    text: &str,
) {
    assert_eq!(error.message(), message);
    assert_eq!(error.file(), file);
    assert_eq!(primary_text(files, error), text);
}

/// Asserts the single related location.
pub fn assert_related(
    files: &[(&str, &str)],
    error: &SchemaError,
    file: &str,
    text: &str,
    message: &str,
) {
    let related = error.related();
    assert_eq!(related.len(), 1, "{related:?}");
    assert_eq!(related[0].file(), file);
    assert_eq!(&text_of(files, file)[related[0].span()], text);
    assert_eq!(related[0].message(), message);
}

pub const HEAD: &str = "datasource db {\n  provider = \"postgresql\"\n}\n\n";
