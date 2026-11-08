//! The multi-file directives (`part`, `part of`, `import`), returned beside
//! the `Schema` and never part of it (docs/design/multi-file-schemas.md
//! section 2), so `print-ir` and the serialized IR of every existing schema
//! stay byte-identical.

use cratestack_core::SourceSpan;

/// One `part "…"`, `part of "…"` or `import "…"` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDirective {
    /// The path exactly as written between the quotes.
    pub path: String,
    /// Covers exactly the text between the quotes.
    pub path_span: SourceSpan,
    /// The trimmed directive line.
    pub span: SourceSpan,
}

/// Every multi-file directive one file declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDirectives {
    /// `part "…"`, in source order.
    pub parts: Vec<PathDirective>,
    /// `part of "…"`.
    pub part_of: Option<PathDirective>,
    /// `import "…"`, in source order.
    pub imports: Vec<PathDirective>,
    /// The `transport` line, if written: `Schema` stores only the style (#993).
    pub(crate) transport_span: Option<SourceSpan>,
}

impl FileDirectives {
    /// True when the file declares no `part`, `part of` or `import`.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.part_of.is_none() && self.imports.is_empty()
    }

    /// The span of the file's `transport` line, if it has one.
    pub fn transport_span(&self) -> Option<SourceSpan> {
        self.transport_span
    }
}
