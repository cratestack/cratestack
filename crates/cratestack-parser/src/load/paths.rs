//! Path rules (M15) and the lexical path arithmetic the loader needs:
//! resolving a directive's path, and showing a file relative to the root.

use std::path::{Component, Path, PathBuf};

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

/// `rel` against the directory of `from_file`, with `.` and `..` folded.
pub(crate) fn resolve(from_file: &Path, rel: &str) -> PathBuf {
    fold(&from_file.parent().unwrap_or(Path::new("")).join(rel))
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

/// The code frame's path for `file_abs`: the root's display path with the
/// directory part joined to the lexical path from the root's directory.
pub(crate) fn display_for(root_display: &str, root_abs: &Path, file_abs: &Path) -> String {
    if file_abs == root_abs {
        return root_display.to_owned();
    }
    let relative = lexical_relative(parent_of(root_abs), file_abs);
    match Path::new(root_display).parent() {
        Some(dir) if !dir.as_os_str().is_empty() => fold(&dir.join(relative)).display().to_string(),
        _ => relative,
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
