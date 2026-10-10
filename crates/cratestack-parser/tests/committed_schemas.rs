//! The census behind ADR 0019 D5, as a test that walks the tree: every
//! committed `.cstack` file keeps its parse result.
//!
//! The field-attribute lists are closed (`validate::field_attributes`), so a
//! committed schema that carries a name its declaration kind does not accept
//! would stop parsing. This walks every `.cstack` in the repository and
//! requires that each one parses, except the few fixtures that are negative
//! on purpose, which must keep failing with the message recorded below. It
//! hard-codes no file count: adding a fixture that parses needs no edit
//! here, and adding one that does not must be declared in
//! [`NEGATIVE_FIXTURES`]. A declared fixture that now parses, or no longer
//! exists, is a failure too, so the list cannot go stale.
//!
//! Counts are printed (`cargo test -p cratestack-parser --test
//! committed_schemas -- --nocapture`). A packaged crate has no repository
//! around it, so the test returns without checking anything there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `(path, the exact message it fails with)`: schemas that are refused on
/// purpose, recorded from the parser as it was before field attributes
/// became closed lists. Each is a test input for some other rule.
const NEGATIVE_FIXTURES: &[(&str, &str)] = &[
    (
        "crates/cratestack-macros/tests/fixtures/mcp_stream_tool.cstack",
        "`@mcp(tool)` on procedure `ticks` exposes a `@stream` procedure: an MCP tool returns a \
         single result, so a streaming procedure cannot be a tool (ADR 0002 Q8). Remove \
         `@mcp(tool)`, or expose a non-streaming procedure instead",
    ),
    (
        "crates/cratestack-macros/tests/fixtures/semantic_error_duplicate_field.cstack",
        "duplicate field `name` on model `User`",
    ),
    (
        "crates/cratestack-macros/tests/fixtures/semantic_error_duplicate_model.cstack",
        "duplicate model name `User`",
    ),
    (
        "crates/cratestack-macros/tests/fixtures/semantic_error_malformed_policy.cstack",
        "model `User` writes `@@allow(\"read\", (banned)`: its argument list is never closed on \
         this line. The generator reads a rule only when it is written `@@allow(\"action\", \
         expression)` and silently skips anything else, so this rule would not be applied; it is \
         refused",
    ),
    (
        "crates/cratestack-macros/tests/fixtures/semantic_error_status_out_of_range.cstack",
        "procedure `broken` @status(404) is outside the allowed 2xx range 200..=299 — non-2xx \
         status is CratestackError's error-mapping's job, not @status's",
    ),
    (
        "crates/cratestack-macros/tests/fixtures/semantic_error_status_under_rpc_transport.cstack",
        "procedure `submit` declares @status, which is a REST-only attribute, but this schema \
         declares `transport rpc` — RPC unary dispatch shares the same handler REST uses, so \
         @status would silently change the RPC response's HTTP status; remove @status from this \
         procedure or switch the schema back to `transport rest`",
    ),
    (
        "crates/cratestack-macros/tests/fixtures/semantic_error_unknown_relation.cstack",
        "relation field `author` on model `Post` references unknown local field `unknownField`",
    ),
    (
        "packages/cratestack-vscode/test/fixtures/invalid-relation.cstack",
        "relation field `author` on model `Post` references unknown local field `ownerId`",
    ),
];

fn repository_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.join("crates/cratestack-macros/src")
        .is_dir()
        .then_some(root)
}

/// Every committed `.cstack`, as repo-relative `/`-separated paths. `git
/// ls-files` when there is a git checkout; otherwise a walk that skips build
/// output, so a source tarball is covered too.
fn committed_schemas(root: &Path) -> Vec<String> {
    let listed = Command::new("git")
        .args(["ls-files", "-z", "--", "*.cstack"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success() && root.join(".git").exists());
    let mut found: Vec<String> = match listed {
        Some(output) => String::from_utf8(output.stdout)
            .expect("git prints UTF-8 paths")
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect(),
        None => {
            let mut walked = Vec::new();
            walk(root, root, &mut walked);
            walked
        }
    };
    found.sort();
    found
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
    const SKIPPED: &[&str] = &["target", "node_modules", ".git", ".claude", "dist"];
    for entry in std::fs::read_dir(dir)
        .expect("readable directory")
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !SKIPPED.iter().any(|skipped| name == *skipped) {
                walk(root, &path, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "cstack") {
            let relative = path.strip_prefix(root).expect("under the root");
            found.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_committed_schema_keeps_its_parse_result() {
    let Some(root) = repository_root() else {
        return;
    };
    let files = committed_schemas(&root);
    let mut unexpected = Vec::new();
    let mut parsed = 0usize;
    let mut refused = BTreeMap::new();
    let mut attributes: BTreeMap<(&str, String), usize> = BTreeMap::new();
    for path in &files {
        let source = std::fs::read_to_string(root.join(path)).expect("readable schema");
        match cratestack_parser::parse_schema_named(path, &source) {
            Ok(schema) => {
                parsed += 1;
                tally(&schema, &mut attributes);
            }
            Err(error) => {
                refused.insert(path.as_str(), error.to_string());
            }
        }
    }
    for (path, message) in &refused {
        match NEGATIVE_FIXTURES
            .iter()
            .find(|(declared, _)| declared == path)
        {
            Some((_, expected)) if expected == message => {}
            Some((_, expected)) => unexpected.push(format!(
                "{path}: refused with a different message\n    expected: {expected}\n    \
                 actual:   {message}"
            )),
            None => unexpected.push(format!("{path}: no longer parses: {message}")),
        }
    }
    for (path, _) in NEGATIVE_FIXTURES {
        if !files.iter().any(|file| file == path) {
            unexpected.push(format!("{path}: declared negative but not committed"));
        } else if !refused.contains_key(path) {
            unexpected.push(format!("{path}: declared negative but it parses"));
        }
    }
    eprintln!(
        "committed .cstack files: {}; parse: {parsed}; negative fixtures: {}",
        files.len(),
        refused.len()
    );
    for ((kind, name), count) in &attributes {
        eprintln!("  field attribute  {kind:6} {name:13} x{count}");
    }
    assert!(
        unexpected.is_empty(),
        "committed schemas changed their parse result:\n{}",
        unexpected.join("\n")
    );
}

/// Counts every field attribute by the kind of declaration that carries it
/// (a model's count includes the fields it took from a mixin).
fn tally(schema: &cratestack_core::Schema, counts: &mut BTreeMap<(&'static str, String), usize>) {
    let mut add = |kind: &'static str, fields: &[cratestack_core::Field]| {
        for attribute in fields.iter().flat_map(|field| &field.attributes) {
            let raw = attribute.raw.trim_start_matches('@');
            let end = raw
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(raw.len());
            *counts
                .entry((kind, format!("@{}", &raw[..end])))
                .or_default() += 1;
        }
    };
    schema.models.iter().for_each(|m| add("model", &m.fields));
    schema.views.iter().for_each(|v| add("view", &v.fields));
    schema.mixins.iter().for_each(|m| add("mixin", &m.fields));
    schema.types.iter().for_each(|t| add("type", &t.fields));
    schema.auth.iter().for_each(|a| add("auth", &a.fields));
}
