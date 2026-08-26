//! `--run-build-runner` (issue #303): after `generate-dart` writes a
//! package to `--out`, optionally shell out to
//! `dart run build_runner build --delete-conflicting-outputs` in that
//! directory. Every preset needs this since issue #668 phase 2/3: every
//! generated data class carries a `@CratestackBuilder(...)` annotation
//! that `package:cratestack_builder` expands into working `{Class}Builder`
//! code; a `--preset riverpod` package additionally needs the step for its
//! own `@riverpod` annotations, expanded into working `.g.dart` code —
//! without this step, the generated package doesn't compile, let alone
//! analyze.
//!
//! Kept as its own `thiserror` enum rather than folded into the rest of
//! this crate's `anyhow`-based handlers (see `cli_handlers.rs`) because
//! each failure mode needs a distinct, prescriptive message — naming the
//! missing tool and the exact manual command — and `thiserror`'s
//! `#[error(...)]` is the clearer way to pin that wording down. `?`
//! converts either variant into the ambient `anyhow::Result` at the call
//! site via the blanket `std::error::Error` conversion.
//!
//! Deliberately does *not* run `dart pub get`/`flutter pub get` first —
//! the acceptance criteria specify exactly one command
//! (`dart run build_runner build --delete-conflicting-outputs`), and
//! `dart run` fetches missing dependencies itself before running. If
//! that ever stops being true for a given SDK, the failure surfaces as a
//! `BuildRunnerError::Failed` with the real `dart`/`pub` output already
//! printed (stdio is inherited, never captured-and-swallowed), so the
//! user isn't left guessing.
use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub(crate) enum BuildRunnerError {
    /// `Command::status` returned `io::ErrorKind::NotFound` — no `dart`
    /// executable resolved on `PATH`. Names the exact manual command so
    /// dropping `--run-build-runner` isn't a dead end.
    #[error(
        "`--run-build-runner` was passed, but no Dart SDK was found on PATH (looked for the \
         `dart` executable). Install the Dart or Flutter SDK \
         (https://docs.flutter.dev/get-started/install), make sure it's on PATH, and re-run — \
         or drop `--run-build-runner` and run this yourself from '{out}':\n\n  \
         dart run build_runner build --delete-conflicting-outputs\n"
    )]
    DartNotFound { out: String },
    /// `dart` resolved but the process couldn't be spawned for some
    /// other reason (permissions, etc.) — distinct from `DartNotFound`
    /// so the message doesn't falsely claim the SDK is missing.
    #[error(
        "failed to launch `dart run build_runner build --delete-conflicting-outputs` in '{out}': {source}"
    )]
    Spawn {
        out: String,
        #[source]
        source: std::io::Error,
    },
    /// The process ran and exited non-zero. Its stdout/stderr were
    /// already inherited straight through to this process's own, so the
    /// real `build_runner`/`riverpod_generator` error is already visible
    /// above this message by the time it's printed.
    #[error(
        "`dart run build_runner build --delete-conflicting-outputs` failed in '{out}' ({status}) \
         — see its output above for the underlying build_runner/riverpod_generator error"
    )]
    Failed {
        out: String,
        status: std::process::ExitStatus,
    },
}

/// Runs `dart run build_runner build --delete-conflicting-outputs` with
/// `out` as the working directory. `out` should already contain a
/// freshly-written generated package (i.e. called after
/// `write_generated_files`, never in `--check` mode).
pub(crate) fn run_build_runner(out: &Path) -> Result<(), BuildRunnerError> {
    spawn_build_runner(out, "dart")
}

/// `program` is injectable purely so tests can prove `DartNotFound`/
/// `Failed` actually fire for real (a genuinely missing executable, a
/// genuinely non-zero exit), rather than only asserting on hand-built
/// `BuildRunnerError` values' `Display` output. Production code only
/// ever calls this via `run_build_runner`, always with `"dart"`.
fn spawn_build_runner(out: &Path, program: &str) -> Result<(), BuildRunnerError> {
    let out_display = out.display().to_string();
    let status = Command::new(program)
        .args([
            "run",
            "build_runner",
            "build",
            "--delete-conflicting-outputs",
        ])
        .current_dir(out)
        .status()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                BuildRunnerError::DartNotFound {
                    out: out_display.clone(),
                }
            } else {
                BuildRunnerError::Spawn {
                    out: out_display.clone(),
                    source,
                }
            }
        })?;

    if !status.success() {
        return Err(BuildRunnerError::Failed {
            out: out_display,
            status,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use tempfile::TempDir;

    use super::{BuildRunnerError, spawn_build_runner};

    #[test]
    fn dart_not_found_message_names_the_tool_and_manual_command() {
        let error = BuildRunnerError::DartNotFound {
            out: "/tmp/client".to_owned(),
        };
        let message = error.to_string();
        assert!(message.contains("no Dart SDK was found on PATH"));
        assert!(message.contains("dart run build_runner build --delete-conflicting-outputs"));
        assert!(message.contains("/tmp/client"));
    }

    #[test]
    fn spawn_error_does_not_claim_the_sdk_is_missing() {
        let error = BuildRunnerError::Spawn {
            out: "/tmp/client".to_owned(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        };
        let message = error.to_string();
        assert!(!message.contains("no Dart SDK was found"));
        assert!(message.contains("failed to launch"));
    }

    /// Not just a `Display` assertion on a hand-built error: this
    /// actually attempts to spawn a program guaranteed not to exist and
    /// confirms the real `io::ErrorKind::NotFound` gets classified as
    /// `DartNotFound`, not `Spawn` — the exact "did you only test the
    /// happy path?" check this story's process section calls for.
    #[test]
    fn a_genuinely_missing_program_is_reported_as_dart_not_found() {
        let dir = TempDir::new().expect("tempdir");
        let error = spawn_build_runner(
            dir.path(),
            "cratestack-cli-test-definitely-not-a-real-binary-9f3c2b",
        )
        .expect_err("a nonexistent program must fail to spawn");
        assert!(
            matches!(error, BuildRunnerError::DartNotFound { .. }),
            "expected DartNotFound, got {error:?}"
        );
    }

    /// Real non-zero exit, not a simulated one: `false` always exits 1
    /// on any POSIX system, so this proves `Failed` actually fires (and
    /// carries the real exit status) rather than only asserting on its
    /// `Display` text.
    #[cfg(unix)]
    #[test]
    fn a_real_nonzero_exit_is_reported_as_failed() {
        let dir = TempDir::new().expect("tempdir");
        let error =
            spawn_build_runner(dir.path(), "false").expect_err("`false` always exits non-zero");
        match error {
            BuildRunnerError::Failed { status, .. } => {
                assert_eq!(status.code(), Some(1));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// The success path: a real process that exits 0 must not be
    /// reported as any error variant.
    #[cfg(unix)]
    #[test]
    fn a_real_zero_exit_succeeds() {
        let dir = TempDir::new().expect("tempdir");
        spawn_build_runner(dir.path(), "true").expect("`true` always exits zero");
    }
}
