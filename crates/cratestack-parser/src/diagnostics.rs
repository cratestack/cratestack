use std::ops::Range;

use ariadne::{Color, Label, Report, ReportKind, Source};
use cratestack_core::SourceSpan;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct SchemaError {
    message: String,
    span: Range<usize>,
    line: usize,
}

impl SchemaError {
    pub(crate) fn new(message: impl Into<String>, span: Range<usize>, line: usize) -> Self {
        Self {
            message: message.into(),
            span,
            line,
        }
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

    pub fn render(&self, path: &str, source: &str) -> String {
        let mut output = Vec::new();
        Report::build(ReportKind::Error, (path.to_owned(), self.span.clone()))
            .with_message(&self.message)
            .with_label(
                Label::new((path.to_owned(), self.span.clone()))
                    .with_message(&self.message)
                    .with_color(Color::Red),
            )
            .finish()
            .write((path.to_owned(), Source::from(source)), &mut output)
            .expect("diagnostic rendering should succeed");

        String::from_utf8(output).expect("ariadne should emit utf-8")
    }
}

pub(crate) fn span_error(message: impl Into<String>, span: SourceSpan) -> SchemaError {
    SchemaError::new(message, span.start..span.end, span.line)
}
