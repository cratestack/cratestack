//! A lenient scan for the files a `.cstack` source points at.
//!
//! Tooling (the LSP's rename guard) must learn "who imports me" from files that
//! may be half-typed or invalid, where a full parse would fail and say nothing.
//! The line grammar lives in `parse::directives`; this only walks lines and
//! reuses it, so there is one definition of what a directive line is.

use crate::line_helpers::collect_lines_at;
use crate::parse::{directive_keyword, parse_path_directive};

/// Which directive a target was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    /// `part "…"`.
    Part,
    /// `part of "…"`.
    PartOf,
    /// `import "…"`.
    Import,
}

/// Every well-formed `part`, `part of` and `import` target in `source`, in
/// order, paths as written. Malformed directive lines are skipped, and nothing
/// else in the file is looked at, so this never fails.
pub fn directive_targets(source: &str) -> Vec<(DirectiveKind, String)> {
    collect_lines_at(source, 0)
        .iter()
        .filter_map(|line| {
            let keyword = directive_keyword(line.trimmed)?;
            let directive = parse_path_directive(line, keyword).ok()?;
            let kind = match keyword {
                "part of" => DirectiveKind::PartOf,
                "part" => DirectiveKind::Part,
                _ => DirectiveKind::Import,
            };
            Some((kind, directive.path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_targets_in_order_and_ignores_comments_and_malformed_lines() {
        let source = "// import \"no.cstack\"\npart of \"main.cstack\"\n  import \"a/b.cstack\"\n\
                      part \"p.cstack\"\nimport nothing\npartial \"x.cstack\"\nmodel M {\n}\n";
        assert_eq!(
            directive_targets(source),
            vec![
                (DirectiveKind::PartOf, "main.cstack".to_owned()),
                (DirectiveKind::Import, "a/b.cstack".to_owned()),
                (DirectiveKind::Part, "p.cstack".to_owned()),
            ]
        );
    }
}
