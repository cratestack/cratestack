//! Path rules (M15) and the lexical path arithmetic the loader needs:
//! resolving a directive's path, and showing a file relative to the root.

use std::path::{Component, Path, PathBuf};

use super::sources::canonical_schema_path;
use crate::diagnostics::SchemaError;
use crate::directives::PathDirective;

/// M15: a directive path is relative, written with `/`, and ends in `.cstack`.
pub(crate) fn check_directive_path(
    directive: &str,
    path_directive: &PathDirective,
) -> Result<(), SchemaError> {
    let path = path_directive.path.as_str();
    let acceptable = !path.is_empty()
        && !path.starts_with('/')
        && !Path::new(path).is_absolute()
        && !path.contains('\\')
        && path.ends_with(".cstack")
        && !(path == ".cstack" || path.ends_with("/.cstack"));
    if acceptable {
        return Ok(());
    }
    let span = path_directive.path_span;
    Err(SchemaError::new(
        format!(
            "`{directive}` path `{path}` must be a relative path written with `/` and ending in \
             `.cstack`"
        ),
        span.start..span.end,
        span.line,
    ))
}

/// The canonical path `rel` names from `from_file`: the one rule for every
/// directive (docs/design/multi-file-schemas.md section 5, Q3). `rel` is joined
/// to the directory of `from_file`'s canonical path and left to the operating
/// system to resolve, so `..` after a symlinked directory means what it means
/// to a shell, and a file reached through a symlink resolves its own
/// directives from where it really is.
pub(crate) fn resolve(from_file: &Path, rel: &str) -> PathBuf {
    let base = canonical_schema_path(from_file);
    canonical_schema_path(&base.parent().unwrap_or(Path::new("")).join(rel))
}

/// Folds `.` and `..` away without touching the filesystem.
pub(crate) fn fold(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// `to` as seen from the directory `from_dir`, `/`-separated, `..` allowed.
/// Both are absolute and folded.
pub(crate) fn lexical_relative(from_dir: &Path, to: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

fn parent_of(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new(""))
}

/// The code frame's path for `file_abs` (canonical): the root's display
/// directory joined to the path from the root's real directory. The display
/// directory is the one the root was named by, unless that does not lead to the
/// root's real directory (the root is a symlink), in which case it is the real
/// one, so that a displayed path always names a file that exists.
pub(crate) fn display_for(root_display: &str, root_abs: &Path, file_abs: &Path) -> String {
    if file_abs == root_abs {
        return root_display.to_owned();
    }
    let relative = lexical_relative(parent_of(root_abs), file_abs);
    let written = Path::new(root_display).parent().unwrap_or(Path::new(""));
    let probe = if written.as_os_str().is_empty() {
        Path::new(".")
    } else {
        written
    };
    match std::fs::canonicalize(probe) {
        Ok(real) if real != parent_of(root_abs) && parent_of(root_abs).is_dir() => {
            fold(&parent_of(root_abs).join(relative))
                .display()
                .to_string()
        }
        _ if written.as_os_str().is_empty() => relative,
        _ => fold(&written.join(relative)).display().to_string(),
    }
}

/// The path shown in messages: relative to the root's directory, the root
/// itself by its file name.
pub(crate) fn root_relative(root_abs: &Path, file_abs: &Path) -> String {
    if file_abs == root_abs {
        return root_abs
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    lexical_relative(parent_of(root_abs), file_abs)
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod paths_tests;
