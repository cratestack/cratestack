use std::ops::Range;
use std::sync::Arc;

use ariadne::{Color, Label, Report, ReportKind, Source};
use cratestack_core::SourceSpan;
use cratestack_core::schema::attribute_text::{escape_for_diagnostic, substitute_for_display};

/// A schema error, identified by which file it came from (cratestack#916).
///
/// `file`/`source_text` start out empty: every constructor below (`new`,
/// `span_error`) fires deep inside parsing/validation, dozens of call sites
/// that have no path in scope and share one property — every error a single
/// parse produces always belongs to the one file that parse was given.
/// Rather than thread a path through every one of those call sites for no
/// behavioral gain, [`SchemaError::with_file`] is applied exactly once, at
/// the boundary where a path *is* known (`parse_schema_named`,
/// `parse_schema_diagnostics`, `parse_schema_file`, `parse_schema_unvalidated`
/// in `entry.rs`). Entry points that take no path tag the error with
/// [`crate::ANONYMOUS_SCHEMA`] and the real source, so a rendered diagnostic
/// always has a code frame — but only the `*_named`/`*_file` paths can name
/// the actual file. Prefer those.
#[derive(Clone, thiserror::Error)]
#[error("{message}")]
pub struct SchemaError {
    message: String,
    span: Range<usize>,
    line: usize,
    file: Arc<str>,
    source_text: Arc<str>,
}

impl std::fmt::Debug for SchemaError {
    /// Deliberately omits `source_text`. A derived `Debug` prints the entire
    /// schema file, so every `.unwrap()` panic on a `Result<_, SchemaError>`
    /// and every `{:?}` log line would dump the whole `.cstack` — a silent
    /// regression with no compile error to catch it. The byte length is
    /// enough to tell "source attached" from "source missing".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaError")
            .field("message", &self.message)
            .field("span", &self.span)
            .field("line", &self.line)
            .field("file", &self.file)
            .field("source_text_len", &self.source_text.len())
            .finish()
    }
}

impl SchemaError {
    /// Every error is built here, so this is where a message that quotes
    /// attribute text or a source line has its control and invisible
    /// characters escaped (`\u{7}`), never written raw to a terminal
    /// (GHSA-69g4-xvcm-vm2j; `cratestack_core::schema::attribute_text::
    /// escape_for_diagnostic`).
    pub(crate) fn new(message: impl Into<String>, span: Range<usize>, line: usize) -> Self {
        Self {
            message: escape_for_diagnostic(&message.into()),
            span,
            line,
            file: Arc::from(""),
            source_text: Arc::from(""),
        }
    }

    /// Attach this error's file identity and that file's source text.
    ///
    /// `source` is an `Arc<str>` the caller already holds (not a fresh
    /// `String`) so tagging every error collected from one parse — several,
    /// with [`crate::parse_schema_diagnostics`] — is a refcount bump each,
    /// not a copy of the whole schema per error.
    pub(crate) fn with_file(mut self, file: &Arc<str>, source: &Arc<str>) -> Self {
        self.file = Arc::clone(file);
        self.source_text = Arc::clone(source);
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn line(&self) -> usize {
        self.line
    }

    /// The file this error belongs to. Errors from the `*_named`/`*_file`
    /// entry points carry the real path; those from the path-less entry
    /// points carry [`crate::ANONYMOUS_SCHEMA`]. Empty only for an error
    /// constructed internally and never passed through [`Self::with_file`],
    /// which no public entry point returns.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Render this error as a human-readable diagnostic.
    ///
    /// Takes no arguments: the error already knows both its file (`self.file`)
    /// and that file's source (`self.source_text`), attached once at the
    /// parse/diagnostics boundary. Before cratestack#916 this took a `(path,
    /// source)` pair supplied by the caller, which had to happen to match the
    /// file the error actually came from — nothing enforced that once more
    /// than one file was involved.
    ///
    /// The code frame quotes the source with each control or invisible
    /// character replaced by one visible stand-in (`␇`, U+FFFD), so the
    /// spans still line up and nothing the schema carries reaches the
    /// terminal raw (`substitute_for_display`).
    pub fn render(&self) -> String {
        let mut output = Vec::new();
        let file = self.file.to_string();
        let shown = substitute_for_display(&self.source_text);
        Report::build(ReportKind::Error, (file.clone(), self.span.clone()))
            .with_message(&self.message)
            .with_label(
                Label::new((file.clone(), self.span.clone()))
                    .with_message(&self.message)
                    .with_color(Color::Red),
            )
            .finish()
            .write((file, Source::from(shown)), &mut output)
            .expect("diagnostic rendering should succeed");

        String::from_utf8(output).expect("ariadne should emit utf-8")
    }
}

pub(crate) fn span_error(message: impl Into<String>, span: SourceSpan) -> SchemaError {
    SchemaError::new(message, span.start..span.end, span.line)
}
