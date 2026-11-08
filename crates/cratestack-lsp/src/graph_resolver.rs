//! Mapping a loader error back to the open file it belongs to.

use std::path::{Path, PathBuf};

use cratestack_parser::{OverlaySources, SchemaError, SchemaSources};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticRelatedInformation, Location, Uri};

use crate::analyze::schema_error_to_diagnostic;
use crate::text::range_from_offsets;

/// Maps an error's file name back to a member, and reads that member's text.
pub(super) struct Resolver<'a> {
    pub(super) overlay: &'a OverlaySources,
    pub(super) members: &'a [(Uri, PathBuf)],
    /// Error file name -> the file's canonical path, from the loader.
    pub(super) named: &'a [(String, PathBuf)],
    pub(super) entry: (&'a Uri, PathBuf),
}

impl Resolver<'_> {
    /// The member a displayed file name stands for. A file the graph could not
    /// read (the root of a part) is not one: the problem is shown on the entry.
    fn member(&self, display: &str) -> (&Uri, &Path) {
        let path = self.named.iter().find(|(name, _)| name == display);
        path.and_then(|(_, path)| self.members.iter().find(|(_, member)| member == path))
            .map(|(uri, member)| (uri, member.as_path()))
            .unwrap_or((self.entry.0, self.entry.1.as_path()))
    }

    fn text(&self, path: &Path) -> String {
        self.overlay.read(path).unwrap_or_default()
    }

    pub(super) fn diagnostic(&self, error: &SchemaError) -> (Uri, Diagnostic) {
        let (uri, path) = self.member(error.file());
        let mut diagnostic = schema_error_to_diagnostic(&self.text(path), error);
        let related: Vec<DiagnosticRelatedInformation> = error
            .related()
            .iter()
            .map(|related| {
                let (uri, path) = self.member(related.file());
                let span = related.span();
                DiagnosticRelatedInformation {
                    location: Location {
                        uri: uri.clone(),
                        range: range_from_offsets(&self.text(path), span.start, span.end),
                    },
                    message: related.message().to_owned(),
                }
            })
            .collect();
        if !related.is_empty() {
            diagnostic.related_information = Some(related);
        }
        (uri.clone(), diagnostic)
    }
}
