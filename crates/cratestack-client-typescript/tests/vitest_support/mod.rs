//! The one way the vitest-driven suites run vitest, so its summary can be
//! matched as plain text.
//!
//! Its own module rather than a file in `support/`: that one is shared by the
//! `tsx`-driven suites, and a binary that compiles it without calling all of
//! it fails `-D warnings` on dead code. The suites that match a vitest summary
//! (`bigint_round_trip.rs`, `bigint_query_keys.rs`) declare `mod vitest_support;`.

use std::path::Path;
use std::process::Command;

/// `npx --yes vitest run --reporter=verbose` in `dir`, with colour pinned off.
///
/// A suite that asserts on the summary (`Test Files  N passed (N)`, with no
/// `skipped` or `todo` next to it) needs that line as one run of plain text.
/// Coloured, vitest splits it with escape codes
/// (`Test Files ESC[22m ESC[1mESC[32mN passedESC[39m…`), so the match fails
/// although every test passed. Whether vitest colours is not stable between
/// machines, so it is pinned here rather than left to the environment:
///
/// * On GitHub Actions (`CI` set) vitest colours its output; in a developer's
///   shell it does not, so this passed locally and failed only in CI. Vitest
///   4 also turns colour off by itself when it detects an AI coding agent
///   (`AI_AGENT`, `CLAUDECODE`), which hides the failure from agent runs too.
/// * `NO_COLOR` is what disables it. `FORCE_COLOR=0` does **not**: vitest's
///   colour library (`tinyrainbow`) treats the variable being present as
///   "force", whatever its value. So `FORCE_COLOR` is removed, not zeroed.
///   Left set next to `NO_COLOR`, Node also prints a warning from every
///   worker that `NO_COLOR` is ignored.
pub fn vitest_command(dir: &Path) -> Command {
    let mut command = Command::new("npx");
    command
        .args(["--yes", "vitest", "run", "--reporter=verbose"])
        .env("NO_COLOR", "1")
        .env_remove("FORCE_COLOR")
        .current_dir(dir);
    command
}
