use std::ops::Range;
use std::sync::Arc;

use ariadne::{Color, Label, Report, ReportKind};
use cratestack_core::schema::attribute_text::substitute_for_display;

use crate::diagnostics::{SchemaError, char_span};

/// A second location an error points at, in its own file: for example the
/// earlier declaration of a name that two files both declare
/// (docs/design/multi-file-schemas.md section 9).
///
/// Like the primary location of a `SchemaError`, `file` and `source_text`
/// start out empty and are attached once, where the loader knows which file
/// a global offset belongs to.
#[derive(Clone)]
pub struct RelatedSpan {
    pub(crate) message: String,
    pub(crate) span: Range<usize>,
    pub(crate) line: usize,
    pub(crate) file: Arc<str>,
    pub(crate) source_text: Arc<str>,
}

impl RelatedSpan {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(message: impl Into<String>, span: Range<usize>, line: usize) -> Self {
        Self {
            message: message.into(),
            span,
            line,
            file: Arc::from(""),
            source_text: Arc::from(""),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Byte span within [`Self::file`]'s text.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn file(&self) -> &str {
        &self.file
    }
}

impl std::fmt::Debug for RelatedSpan {
    /// Omits `source_text`, like `SchemaError`'s `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelatedSpan")
            .field("message", &self.message)
            .field("span", &self.span)
            .field("line", &self.line)
            .field("file", &self.file)
            .finish()
    }
}

impl SchemaError {
    /// The multi-location form of [`Self::render`]: one label per location,
    /// each in its own file's code frame. Only reached when `related` is
    /// non-empty, so every other error renders exactly as before.
    pub(crate) fn render_with_related(&self) -> String {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut register = |file: &str, text: &str| {
            if !files.iter().any(|(name, _)| name == file) {
                files.push((file.to_owned(), substitute_for_display(text)));
            }
        };
        register(&self.file, &self.source_text);
        for related in &self.related {
            register(&related.file, &related.source_text);
        }
        let primary = (
            self.file.to_string(),
            char_span(&self.source_text, &self.span),
        );
        let mut report = Report::build(ReportKind::Error, primary.clone())
            .with_message(&self.message)
            .with_label(
                Label::new(primary)
                    .with_message(&self.message)
                    .with_color(Color::Red),
            );
        for related in &self.related {
            let span = char_span(&related.source_text, &related.span);
            report = report.with_label(
                Label::new((related.file.to_string(), span))
                    .with_message(&related.message)
                    .with_color(Color::Blue),
            );
        }
        let mut output = Vec::new();
        report
            .finish()
            .write(ariadne::sources(files), &mut output)
            .expect("diagnostic rendering should succeed");
        String::from_utf8(output).expect("ariadne should emit utf-8")
    }
}
