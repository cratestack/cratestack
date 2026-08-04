//! Rust keyword classification, shared by `cratestack-parser` (schema-time
//! field-name validation) and `cratestack-macros` (identifier escaping at
//! codegen time) so the two stay in sync — see cratestack#398.
//!
//! A `.cstack` field name is an arbitrary wire-format string; codegen turns
//! it into a Rust identifier verbatim. If that string happens to be a Rust
//! keyword, the naive identifier is not valid Rust. Most keywords have a
//! raw-identifier escape hatch (`r#type`, `r#match`, ...); four do not
//! (`self`, `Self`, `super`, `crate` — rustc rejects `r#self` outright) and
//! must be rejected before codegen ever sees them.

/// Keywords that cannot be represented as a Rust identifier at all, not even
/// as a raw identifier. Per the Rust reference, `r#self`, `r#Self`,
/// `r#super`, and `r#crate` are explicitly disallowed by the compiler —
/// there is no valid spelling, so a field with one of these names must be
/// rejected at schema-parse time rather than escaped.
pub const UNREPRESENTABLE_KEYWORDS: &[&str] = &["self", "Self", "super", "crate"];

/// Every other Rust keyword (strict + reserved, across every edition) that
/// *is* a legal identifier once written as a raw identifier (`r#keyword`).
const RAW_ESCAPABLE_KEYWORDS: &[&str] = &[
    // Strict keywords (2015+)
    "as", "break", "const", "continue", "else", "enum", "extern", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static",
    "struct", "trait", "true", "type", "unsafe", "use", "where", "while",
    // Strict keywords (2018+)
    "async", "await", "dyn", // Reserved keywords (2015+)
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized",
    "virtual", "yield", // Reserved keywords (2018+)
    "try",   // Reserved keywords (2024+)
    "gen",
];

/// `true` if `name` is a Rust keyword that needs handling before it can be
/// emitted as a Rust identifier — either raw-identifier escaping (see
/// [`is_raw_escapable_keyword`]) or outright rejection (see
/// [`is_unrepresentable_keyword`]).
pub fn is_rust_keyword(name: &str) -> bool {
    is_raw_escapable_keyword(name) || is_unrepresentable_keyword(name)
}

/// `true` if `name` is a Rust keyword that can be emitted as `r#name`.
pub fn is_raw_escapable_keyword(name: &str) -> bool {
    RAW_ESCAPABLE_KEYWORDS.contains(&name)
}

/// `true` if `name` is a Rust keyword with no valid identifier spelling at
/// all — `self`, `Self`, `super`, `crate`.
pub fn is_unrepresentable_keyword(name: &str) -> bool {
    UNREPRESENTABLE_KEYWORDS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapable_keywords_are_not_also_unrepresentable() {
        for keyword in RAW_ESCAPABLE_KEYWORDS {
            assert!(
                !is_unrepresentable_keyword(keyword),
                "`{keyword}` listed as both raw-escapable and unrepresentable"
            );
        }
    }

    #[test]
    fn ordinary_identifiers_are_not_keywords() {
        for name in ["user", "email", "created_at", "matches", "typeName"] {
            assert!(!is_rust_keyword(name), "`{name}` should not be a keyword");
        }
    }

    #[test]
    fn ticket_398_keyword_table_is_covered() {
        // cratestack#398's own tested table.
        for keyword in [
            "match", "type", "ref", "move", "impl", "fn", "let", "loop", "box",
        ] {
            assert!(is_raw_escapable_keyword(keyword));
        }
        for keyword in ["self", "crate"] {
            assert!(is_unrepresentable_keyword(keyword));
        }
    }
}
