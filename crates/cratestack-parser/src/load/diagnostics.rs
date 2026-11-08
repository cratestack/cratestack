//! Orchestration: graph, part rules and collisions are a hard stop; then
//! merge, expand mixins and validate the merged schema, attributing every
//! error to its file.

use std::path::{Path, PathBuf};

use super::attribute::attribute;
use super::graph::Graph;
use super::sources::{FsSources, SchemaSources};
use super::{LoadDiagnostics, LoadOptions, LoadedSchema, SchemaFile, collisions, merge};
use crate::diagnostics::SchemaError;
use crate::parse::{ParsedFile, expand_mixins};
use crate::validate::validate_schema_collecting;

/// Loads `path` and reports every independent problem, from several files
/// in one run (docs/design/multi-file-schemas.md section 9).
pub fn load_schema_diagnostics(
    path: impl AsRef<Path>,
    options: &LoadOptions<'_>,
) -> LoadDiagnostics {
    let fs = FsSources;
    let sources: &dyn SchemaSources = options.sources.unwrap_or(&fs);
    let mut graph = Graph::new(sources);
    let loaded = graph.load(path.as_ref(), options.follow_part_of);
    let Graph {
        files, attempted, ..
    } = graph;
    let named: Vec<(String, PathBuf)> = files
        .iter()
        .map(|f| (f.file.display.clone(), f.file.path.clone()))
        .collect();
    let stop = |errors: Vec<SchemaError>| LoadDiagnostics {
        schema: None,
        errors,
        files: attempted.clone(),
        named: named.clone(),
    };
    if let Err(error) = loaded {
        return stop(vec![error]);
    }
    let attribute_all = |error: SchemaError, files: &[SchemaFile]| attribute(error, files.iter());
    if let Err(error) = collisions::check(&files) {
        let infos: Vec<SchemaFile> = files.iter().map(|f| f.file.clone()).collect();
        return stop(vec![attribute_all(error, &infos)]);
    }
    let mut infos = Vec::with_capacity(files.len());
    let mut parsed: Vec<ParsedFile> = Vec::with_capacity(files.len());
    for loaded in files {
        infos.push(loaded.file);
        parsed.push(
            loaded
                .parsed
                .expect("every file parsed once the graph loaded"),
        );
    }
    let mut schema = merge::merge(parsed);
    if let Err(error) = expand_mixins(&mut schema) {
        return stop(vec![attribute_all(error, &infos)]);
    }
    let errors: Vec<SchemaError> = validate_schema_collecting(&infos[0].display, "", &schema)
        .into_iter()
        .map(|error| attribute_all(error, &infos))
        .collect();
    if !errors.is_empty() {
        return stop(errors);
    }
    LoadDiagnostics {
        schema: Some(LoadedSchema {
            schema,
            files: infos,
        }),
        errors: Vec::new(),
        files: attempted,
        named,
    }
}
