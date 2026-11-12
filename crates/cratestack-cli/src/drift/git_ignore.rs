use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which of `candidates` git considers ignored, so they can be excluded
/// from the `Unexpected` arm.
///
/// `--check` exists to keep *committed* generated output in sync with the
/// schema, and a file that isn't committed cannot drift from it. Without
/// this, running the check after `flutter pub get` / `build_runner` reports
/// every `.dart_tool/` entry, `*.g.dart`, `*.mapper.dart` and `pubspec.lock`
/// as drift while the generated content matches perfectly (cratestack#659) —
/// and the workaround for that false failure is indistinguishable from the
/// workaround for a real one.
///
/// Delegating to `git check-ignore` rather than reimplementing gitignore
/// matching (anchoring, negation, `**`, nested `.gitignore` files) or taking
/// on the `ignore` crate's `globset`/`regex-automata` subtree: git is
/// necessarily present for a checkout that has a `.gitignore` to honour in
/// the first place.
///
/// Returns empty — i.e. preserves the previous behaviour exactly — when
/// `out` is not inside a work tree, when git is unavailable, or when the
/// call fails. It also returns empty when `out` is *itself* ignored (a
/// scratch directory under `target/`, say): everything beneath such a
/// directory is ignored, so filtering on that basis would silently disable
/// stale-file detection altogether rather than refine it.
pub(super) fn git_ignored(out: &Path, candidates: &[PathBuf]) -> HashSet<PathBuf> {
    let empty = HashSet::new();
    if candidates.is_empty() || !is_tracked_territory(out) {
        return empty;
    }

    // Feed ABSOLUTE paths. `git -C <out>` resolves relative arguments
    // against `out`, not against our CWD — so a relative `--out` (which is
    // what `just regen-examples` passes) turned every candidate into a
    // nonexistent `<out>/<out>/...`. Basename rules like `*.g.dart` still
    // matched, path-anchored ones like `client/.dart_tool/` did not, and the
    // filter silently did about a third of its job. `absolute` rather than
    // `canonicalize` so this does not depend on the path existing or resolve
    // symlinks out from under the caller's own spelling of it.
    let mut absolute = Vec::with_capacity(candidates.len());
    let mut stdin = Vec::new();
    for path in candidates {
        let Ok(abs) = std::path::absolute(path) else {
            continue;
        };
        stdin.extend_from_slice(abs.as_os_str().as_encoded_bytes());
        stdin.push(0);
        absolute.push((abs, path));
    }
    if stdin.is_empty() {
        return empty;
    }

    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return empty;
    };
    if let Some(pipe) = child.stdin.as_mut() {
        // A broken pipe here just means git bailed early; the empty/partial
        // stdout below is then handled like any other inconclusive result.
        let _ = pipe.write_all(&stdin);
    }
    drop(child.stdin.take());
    let Ok(output) = child.wait_with_output() else {
        return empty;
    };
    // Exit 0 = some paths ignored, 1 = none ignored (not an error),
    // 128 = fatal. `out` is known to be in a work tree (checked above), so
    // 128 here means git could not answer for *some* path in the batch —
    // e.g. "beyond a symbolic link" (cratestack#1235) — and aborts for ALL of
    // them. That is inconclusive, not "nothing is ignored": ask per path, so
    // one unanswerable path cannot switch the filter off for the rest.
    match output.status.code() {
        Some(0) => {}
        Some(1) => return empty,
        _ => return absolute_each_ignored(out, &absolute),
    }

    // git echoes back exactly the strings it was given, so map the absolute
    // paths in its answer back to the caller's own spelling.
    let reported = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(String::from_utf8_lossy(line).into_owned()))
        .collect::<HashSet<_>>();

    absolute
        .into_iter()
        .filter(|(abs, _)| reported.contains(abs))
        .map(|(_, original)| original.clone())
        .collect()
}

/// Whether `out` sits in a git work tree *and* is not itself ignored.
fn is_tracked_territory(out: &Path) -> bool {
    let inside = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !matches!(inside, Ok(status) if status.success()) {
        return false;
    }
    // `check-ignore -q` exits 0 when the path IS ignored.
    let self_ignored = Command::new("git")
        .arg("-C")
        .arg(out)
        .args(["check-ignore", "-q", "."])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    !matches!(self_ignored, Ok(status) if status.success())
}

/// Per-path fallback for a batch `check-ignore` that exited 128. A path git
/// still cannot answer for (exit 128 again) counts as not ignored, so it is
/// reported rather than silently hidden.
fn absolute_each_ignored(out: &Path, absolute: &[(PathBuf, &PathBuf)]) -> HashSet<PathBuf> {
    absolute
        .iter()
        .filter(|(abs, _)| {
            Command::new("git")
                .arg("-C")
                .arg(out)
                .args(["check-ignore", "-q", "--"])
                .arg(abs)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.code() == Some(0))
        })
        .map(|(_, original)| (*original).clone())
        .collect()
}
