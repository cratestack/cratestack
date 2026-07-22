//! Static regression guard against the cross-binary table-name
//! collision bug fixed in this PR: `cargo test -p cratestack-pg` runs
//! every file in this directory as its own OS process against the
//! same shared test Postgres, and `support::pg::serial_guard()`'s
//! mutex only serializes tests *within* one binary — not across
//! binaries. Two files that derive the same physical table name from
//! *different* `.cstack` fixtures can race DROP/CREATE/INSERT on that
//! table and fail nondeterministically (this reproduced for real:
//! `customers`, `users`, and `posts`, all fixed alongside this test).
//!
//! This scans every other file in this directory for `CREATE TABLE
//! <name>` and `include_server_schema!("<fixture>", ...)`, then fails
//! if a table name is produced by files that don't all share the
//! exact same fixture set. Sharing one fixture is deliberate,
//! documented reuse (see `banking_include_sugar.rs`,
//! `banking_builder_extensions.rs`) and is allowed; two *different*
//! fixtures independently landing on the same default
//! `pluralize(snake_case(model.name))` table name is the actual bug
//! class this guards against.
//!
//! Pure static analysis — no database required, always runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[test]
fn integration_test_table_names_do_not_collide_across_unrelated_fixtures() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut contributors_by_table: BTreeMap<String, Vec<(String, BTreeSet<String>)>> =
        BTreeMap::new();

    for entry in std::fs::read_dir(&tests_dir).expect("tests dir should be readable") {
        let path = entry.expect("dir entry should be readable").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("test file should have a utf-8 name")
            .to_owned();
        if file_name == "fixture_table_names.rs" {
            continue;
        }

        let source = std::fs::read_to_string(&path).expect("test source should be readable");
        let fixtures = extract_fixture_paths(&source);

        for table in extract_create_table_names(&source) {
            contributors_by_table
                .entry(table)
                .or_default()
                .push((file_name.clone(), fixtures.clone()));
        }
    }

    let mut collisions = Vec::new();
    for (table, contributors) in &contributors_by_table {
        if contributors.len() < 2 {
            continue;
        }
        // Allowed iff every contributing file shares one identical,
        // non-empty fixture set with every other contributor — i.e.
        // they all `include_server_schema!` the same `.cstack` file(s)
        // on purpose, rather than coincidentally landing on the same
        // table name from unrelated fixtures.
        let first_fixtures = &contributors[0].1;
        let deliberately_shared =
            !first_fixtures.is_empty() && contributors.iter().all(|(_, f)| f == first_fixtures);
        if !deliberately_shared {
            let files: Vec<&str> = contributors.iter().map(|(f, _)| f.as_str()).collect();
            collisions.push(format!("  `{table}` — declared by: {}", files.join(", ")));
        }
    }

    assert!(
        collisions.is_empty(),
        "table name(s) collide across unrelated integration-test fixtures. \
         cargo test -p cratestack-pg runs each file below as its own OS process \
         against the same shared Postgres, so DROP/CREATE/INSERT on a shared \
         table name races nondeterministically across binaries. Rename the \
         underlying model in one of the colliding files — table name is always \
         pluralize(snake_case(model.name)); there's no @@map-style override — \
         or, if the reuse is deliberate, make both files include_server_schema! \
         the exact same fixture path so this check recognizes it as shared:\n{}",
        collisions.join("\n"),
    );
}

/// Every `"<path>"` string literal immediately following an
/// `include_server_schema!(` call, tolerant of both single-line and
/// rustfmt-wrapped multi-line invocations.
fn extract_fixture_paths(source: &str) -> BTreeSet<String> {
    let marker = "include_server_schema!(";
    let mut out = BTreeSet::new();
    let mut rest = source;
    while let Some(start) = rest.find(marker) {
        let after_marker = &rest[start + marker.len()..];
        let Some(quote_start) = after_marker.find('"') else {
            rest = after_marker;
            continue;
        };
        let after_quote = &after_marker[quote_start + 1..];
        let Some(quote_end) = after_quote.find('"') else {
            rest = after_marker;
            continue;
        };
        out.insert(after_quote[..quote_end].to_owned());
        rest = &after_quote[quote_end + 1..];
    }
    out
}

/// Every identifier following a `CREATE TABLE` (optionally `IF NOT
/// EXISTS`) in a raw SQL string literal.
fn extract_create_table_names(source: &str) -> BTreeSet<String> {
    let marker = "CREATE TABLE ";
    let mut out = BTreeSet::new();
    let mut rest = source;
    while let Some(start) = rest.find(marker) {
        let after = &rest[start + marker.len()..];
        let after = after.strip_prefix("IF NOT EXISTS ").unwrap_or(after);
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.insert(name);
        }
        rest = after;
    }
    out
}
