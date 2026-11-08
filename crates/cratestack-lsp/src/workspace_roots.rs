//! The workspace roots the client announced at `initialize`.

use std::path::PathBuf;
use std::sync::OnceLock;

use cratestack_parser::canonical_schema_path;
use tower_lsp_server::ls_types::InitializeParams;

/// Set once, at `initialize`. Empty when the client sent no folder.
#[derive(Default)]
pub(crate) struct WorkspaceRoots(OnceLock<Vec<PathBuf>>);

impl WorkspaceRoots {
    pub(crate) fn set(&self, roots: Vec<PathBuf>) {
        let _ = self.0.set(roots);
    }

    pub(crate) fn get(&self) -> &[PathBuf] {
        self.0.get().map_or(&[], Vec::as_slice)
    }
}

/// `workspace_folders` when sent, else the deprecated `root_uri`; canonical.
pub(crate) fn roots_from(params: &InitializeParams) -> Vec<PathBuf> {
    #[allow(deprecated)]
    let root_uri = params.root_uri.as_ref();
    let uris: Vec<_> = match &params.workspace_folders {
        Some(folders) if !folders.is_empty() => folders.iter().map(|f| &f.uri).collect(),
        _ => root_uri.into_iter().collect(),
    };
    uris.into_iter()
        .filter_map(|uri| uri.to_file_path())
        .map(|path| canonical_schema_path(&path))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tower_lsp_server::ls_types::{Uri, WorkspaceFolder};

    use super::*;

    fn uri(path: &str) -> Uri {
        Uri::from_str(path).expect("uri")
    }

    #[test]
    fn workspace_folders_win_over_root_uri_and_root_uri_is_the_fallback() {
        let mut params = InitializeParams::default();
        assert!(roots_from(&params).is_empty());
        #[allow(deprecated)]
        {
            params.root_uri = Some(uri("file:///legacy"));
        }
        assert_eq!(roots_from(&params), vec![PathBuf::from("/legacy")]);
        params.workspace_folders = Some(vec![WorkspaceFolder {
            uri: uri("file:///ws"),
            name: "ws".into(),
        }]);
        assert_eq!(roots_from(&params), vec![PathBuf::from("/ws")]);
    }
}
