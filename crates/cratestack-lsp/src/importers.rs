//! Is this file another schema's `import` or `part`?
//!
//! A shared file that holds only `type`/`enum`/`mixin` carries no directive of
//! its own, so opened alone it looks like an ordinary single-file schema while
//! every importer depends on the names in it. Renaming there would silently
//! break them, so rename is refused when any `.cstack` file in the client's
//! workspace points at the document.
//!
//! Limits, by construction: only the workspace roots the client announced at
//! `initialize` are searched, so with no roots (or an importer outside them)
//! nothing can be known and rename behaves as before.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use cratestack_parser::{canonical_schema_path, directive_targets};
use tower_lsp_server::ls_types::Uri;

use crate::rename_error::RenameError;
use crate::state::{Backend, DocumentState};

/// The walk gives up past this many `.cstack` files or directories. Giving up
/// refuses the rename: an unfinished search cannot show that nobody imports
/// the file, and a refused rename is recoverable where a broken importer is not.
const WALK_BUDGET: usize = 10_000;

const SKIPPED_DIRS: &[&str] = &["target", "node_modules"];

/// Open buffers by canonical path: their text wins over the disk's.
pub(crate) fn open_texts(documents: &HashMap<Uri, DocumentState>) -> HashMap<PathBuf, String> {
    documents
        .iter()
        .filter_map(|(uri, state)| {
            let path = uri.to_file_path()?;
            Some((canonical_schema_path(&path), state.text.clone()))
        })
        .collect()
}

/// `Err(MultiFile)` when a `.cstack` file under `roots` (or an open buffer)
/// lists `target` as an import or a part. Blocking: walks the filesystem.
pub(crate) fn check_not_imported(
    roots: &[PathBuf],
    target: &Path,
    open: &HashMap<PathBuf, String>,
) -> Result<(), RenameError> {
    if roots.is_empty() {
        return Ok(());
    }
    let target = canonical_schema_path(target);
    let Some(mut candidates) = walk(roots) else {
        return Err(RenameError::MultiFile);
    };
    candidates.extend(open.keys().cloned());
    for file in candidates {
        if file == target {
            continue;
        }
        let text = match open.get(&file) {
            Some(text) => text.clone(),
            None => match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(_) => continue,
            },
        };
        let points_here = directive_targets(&text).iter().any(|(_, rel)| {
            // The loader's rule: relative to the importer's real directory.
            let base = canonical_schema_path(&file);
            canonical_schema_path(&base.parent().unwrap_or(Path::new("")).join(rel)) == target
        });
        if points_here {
            return Err(RenameError::MultiFile);
        }
    }
    Ok(())
}

/// Every `.cstack` file under `roots`, or `None` if the budget ran out.
/// Directories are not followed through symlinks, so a link cycle cannot loop.
fn walk(roots: &[PathBuf]) -> Option<HashSet<PathBuf>> {
    let mut files = HashSet::new();
    let mut dirs_seen = 0usize;
    let mut pending: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = pending.pop() {
        dirs_seen += 1;
        if dirs_seen > WALK_BUDGET {
            return None;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_ref()) {
                    pending.push(entry.path());
                }
            } else if name.ends_with(".cstack") {
                files.insert(canonical_schema_path(&entry.path()));
                if files.len() > WALK_BUDGET {
                    return None;
                }
            }
        }
    }
    Some(files)
}

impl Backend {
    /// Runs [`check_not_imported`] for `uri` off the async executor. The
    /// `documents` lock is held only to copy the open texts, never across the
    /// walk (see `graph_update.rs`).
    pub(crate) async fn check_not_imported(&self, uri: &Uri) -> Result<(), RenameError> {
        let roots = self.workspace_roots.get();
        let Some(path) = uri.to_file_path().filter(|_| !roots.is_empty()) else {
            return Ok(());
        };
        let open = open_texts(&*self.documents.read().await);
        let (roots, path) = (roots.to_vec(), path.into_owned());
        tokio::task::spawn_blocking(move || check_not_imported(&roots, &path, &open))
            .await
            // A panicked walk proved nothing: refuse.
            .unwrap_or(Err(RenameError::MultiFile))
    }
}
