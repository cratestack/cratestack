//! Public lists the parser owns: builtin scalar names and reserved
//! multi-file keywords (split out of `lib.rs`, which is at the line ceiling).

use crate::validate;

/// Canonical scalar type names built into the `.cstack` language (e.g.
/// `String`, `Int`, `Decimal`, ...), including `Page` (which is valid only
/// as a procedure return type — see `validate::type_names::validate_type_ref`
/// — not as a plain field type).
///
/// This is the same list `cratestack-lsp`'s autocompletion and the
/// `cratestack-pg`/`cratestack-sqlite` emitter/decoder round-trip tests
/// assert against, so a new builtin scalar only has to be added here once —
/// see cratestack#232 for why that matters (this list had already silently
/// drifted from the LSP's hand-copied one before this accessor existed).
pub fn builtin_type_names() -> &'static [&'static str] {
    validate::builtin_type_names()
}

/// Words reserved for the multi-file schema grammar. Consumers that accept
/// prospective identifiers, such as the LSP rename flow, must reject this
/// canonical list before producing source the parser cannot read.
pub fn reserved_multi_file_keywords() -> &'static [&'static str] {
    validate::multi_file_schema_keywords()
}
