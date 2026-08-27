//! Attributable failure text for a subprocess these tests spawn.

use std::process::{Command, Output};

/// Renders everything needed to attribute a failed child process, for use as
/// the body of an `assert!` message.
///
/// The exit status is the point (cratestack#738's second comment). A CI run on
/// `bbc8bbfb` panicked with `smoke script failed:` and **both** streams empty,
/// which on that evidence alone is indistinguishable from a genuine assertion
/// failure in the generated TypeScript — a reader could not tell npm's tooling
/// had died from the test's own output. Status and command line are reported
/// even when the streams say nothing; a signal death (no exit code) is called
/// out by name rather than rendered as a bare `-1`.
pub fn command_report(command: &Command, output: &Output) -> String {
    // Reported explicitly rather than relying on `Command`'s `Debug`, whose
    // rendering (currently `cd "…" && "prog" "arg"`) std does not guarantee.
    let cwd = command
        .get_current_dir()
        .map_or_else(|| "<inherited>".to_owned(), |dir| dir.display().to_string());
    let status = match output.status.code() {
        Some(code) => format!("exit code {code}"),
        None => format!("killed by signal ({})", output.status),
    };
    format!(
        "command: {command:?}\ncwd: {cwd}\nstatus: {status}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
