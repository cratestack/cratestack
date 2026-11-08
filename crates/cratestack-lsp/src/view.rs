//! One document's schema, with the address-space arithmetic that makes it
//! usable (design section 12).
//!
//! In a schema split across files, every span in `schema` addresses the merged
//! text, while the editor's buffer is one file of it. `Resolved` carries both
//! and converts between them, so a feature asks "where is this in my file?"
//! rather than comparing a local offset with a global span. For a single file
//! (`base` 0, no graph) every helper is the identity.

use cratestack_core::{Schema, SourceSpan};
use tower_lsp_server::ls_types::{Position, Range, Uri};

use crate::graph::GraphView;
use crate::text::{position_to_offset, range_from_offsets};

pub(crate) struct Resolved<'a> {
    /// This file's text.
    pub(crate) text: &'a str,
    pub(crate) schema: &'a Schema,
    /// Where this file starts in the schema's address space.
    pub(crate) base: usize,
    pub(crate) graph: Option<&'a GraphView>,
    /// The text `schema`'s spans index into: every file, at its base. The
    /// file's own text for a single file.
    pub(crate) global: &'a str,
}

impl<'a> Resolved<'a> {
    #[cfg(test)]
    pub(crate) fn single(text: &'a str, schema: &'a Schema) -> Self {
        Self {
            text,
            schema,
            base: 0,
            graph: None,
            global: text,
        }
    }

    pub(crate) fn to_global(&self, local: usize) -> usize {
        local + self.base
    }

    /// A position in this file as an offset in the schema's address space.
    pub(crate) fn global_offset(&self, position: Position) -> Option<usize> {
        position_to_offset(self.text, position).map(|local| self.to_global(local))
    }

    /// `span` rebased onto this file, when it lies in it.
    pub(crate) fn local_span(&self, span: SourceSpan) -> Option<SourceSpan> {
        if self.graph.is_none() {
            return Some(span);
        }
        (self.base <= span.start && span.end <= self.base + self.text.len()).then(|| SourceSpan {
            start: span.start - self.base,
            end: span.end - self.base,
            line: span.line,
        })
    }

    /// `span` as a range of this file, when it lies in it.
    pub(crate) fn range(&self, span: SourceSpan) -> Option<Range> {
        let local = self.local_span(span)?;
        Some(range_from_offsets(self.text, local.start, local.end))
    }

    /// The file `span` is in, as the URI to answer with, its text and the span
    /// within it. `own` is the URI of the requesting file.
    pub(crate) fn locate(&self, span: SourceSpan, own: &Uri) -> Option<(Uri, &'a str, SourceSpan)> {
        let Some(graph) = self.graph else {
            return Some((own.clone(), self.text, span));
        };
        let (file, start) = graph.locate(span.start)?;
        if file.base == self.base {
            return Some((own.clone(), self.text, self.local_span(span)?));
        }
        let local = SourceSpan {
            start,
            end: span.end.checked_sub(file.base)?,
            line: span.line,
        };
        let uri = Uri::from_file_path(&file.path)?;
        Some((uri, graph_source(graph, file.base)?, local))
    }
}

fn graph_source(graph: &GraphView, base: usize) -> Option<&str> {
    graph
        .files
        .iter()
        .find(|file| file.base == base)
        .map(|file| &*file.source)
}
