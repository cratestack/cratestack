//! A reader tripwire for the drop list. An attribute is wire-neutral only
//! while nothing that shapes a wire type, route or client reads it; the
//! reasons in `DROPPED_ATTRIBUTES` are prose, so this pins, per dropped name,
//! the source files that mention it (comment lines, tests, the parser and
//! `schema/` excluded). A new reader changes the list and fails here: check
//! that it still decides no wire shape, then update the table. Only the
//! `@`-spelled text is searched; a reader matching the bare name
//! (`field_attribute_name(..) == Some("length")`) is not seen, which is why
//! each entry also has a parsed-source test in `cratestack-parser`.

use std::path::{Path, PathBuf};

use super::attrs::DROPPED_ATTRIBUTES;
use super::tests_readers_symbols::SYMBOL_READERS;
use super::tests_readers_table::READERS;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .expect("readable source tree")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn skipped(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    let base = parts.last().copied().unwrap_or_default();
    parts[0] == "cratestack-parser"
        || relative.starts_with("cratestack-core/src/schema/")
        || relative.contains("client_contract")
        || base.starts_with("tests")
        || base.ends_with("_tests.rs")
        || parts[..parts.len() - 1].contains(&"tests")
}

fn mentions(line: &str, name: &str) -> bool {
    let bytes = line.as_bytes();
    line.match_indices(name).any(|(at, _)| {
        let after = bytes.get(at + name.len());
        let word_after = after.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        let single_inside_double = !name.starts_with("@@") && at > 0 && bytes[at - 1] == b'@';
        !word_after && !single_inside_double
    })
}

fn readers_of(name: &str, crates: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).expect("crates dir").flatten() {
        rust_files(&entry.path().join("src"), &mut files);
    }
    let mut found: Vec<String> = files
        .iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(crates).ok()?.to_str()?.replace('\\', "/");
            if skipped(&relative) {
                return None;
            }
            let text = std::fs::read_to_string(path).ok()?;
            let read = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| mentions(l, name));
            read.then_some(relative)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn the_reader_table_covers_exactly_the_drop_list() {
    let table: Vec<&str> = READERS.iter().map(|(n, _)| *n).collect();
    let dropped: Vec<&str> = DROPPED_ATTRIBUTES.iter().map(|(n, _)| *n).collect();
    assert_eq!(table, dropped);
}

#[test]
fn no_dropped_attribute_has_a_new_reader() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let crates = crates.canonicalize().expect("crates dir");
    if !crates.join("cratestack-macros/src").is_dir() {
        return; // a packaged crate has no sibling sources to read
    }
    for (name, pinned) in READERS {
        assert_eq!(
            readers_of(name, &crates),
            *pinned,
            "{name}: its set of readers changed. Decide whether the new reader shapes a wire \
             type, route or client; if so take the attribute off DROPPED_ATTRIBUTES, else \
             update tests_readers_table.rs"
        );
    }
}

#[test]
fn redaction_stays_in_the_audit_writer() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let crates = crates.canonicalize().expect("crates dir");
    if !crates.join("cratestack-macros/src").is_dir() {
        return;
    }
    for (name, pinned) in SYMBOL_READERS {
        assert_eq!(
            readers_of(name, &crates),
            *pinned,
            "{name}: its set of readers changed. `@pii` and `@sensitive` are off the op \
             contract because they only redact the audit log; if this reader redacts a \
             response, error or event body, take both off DROPPED_ATTRIBUTES"
        );
    }
}
