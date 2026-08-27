//! Resolves the pinned `tsx` runner once per target directory, with no
//! `npx` anywhere on the path a test actually takes (cratestack#738).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Pinned, not `tsx@latest`. An unpinned tool inside CI is a dependency whose
/// version changes without a commit here, and the failure it produces lands in
/// a test whose diagnostics are printed only if everything else goes right.
/// 4.23.12 is the latest release as of 2026-08-24.
const TSX_PIN: &str = "tsx@4.23.12";

/// `node` and `npm` — deliberately **not** `npx`, which nothing here invokes
/// any more (cratestack#738). The probe asserts exactly what the harness
/// uses; leaving `npx` in it would be a check on a tool no code path touches.
/// In every environment this guard exists for the three ship together, so the
/// documented skip behaviour in Rust-only CI jobs is unchanged.
pub fn node_toolchain_available() -> bool {
    ["node", "npm"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

/// A `Command` that runs `script` (resolved relative to `working_dir`) under
/// the pinned `tsx`, as `node <cli.mjs> <script>` — which is precisely what
/// npm's `node_modules/.bin/tsx` shim execs, so behaviour is identical to the
/// `npx --yes tsx@… <script>` this replaced.
pub fn tsx_command(working_dir: &Path, script: &str) -> Command {
    let mut command = Command::new("node");
    command.arg(tsx_cli()).arg(script).current_dir(working_dir);
    command
}

fn tsx_cli() -> &'static Path {
    static CLI: OnceLock<PathBuf> = OnceLock::new();
    CLI.get_or_init(resolve).as_path()
}

/// Install-then-atomically-publish, rather than installing in place.
///
/// `CARGO_TARGET_TMPDIR` (`<target-dir>/tmp`) is shared by every test binary
/// and is not cleaned between runs, so the published tree survives across
/// binaries *and* across invocations of `cargo test`: the steady state is a
/// single `is_file()` stat and no npm process at all. It is inside `target/`,
/// so `cargo clean` disposes of it and nothing here needs gitignoring.
///
/// The publish is a `rename` of a fully-populated staging directory, which is
/// what makes concurrency safe *without* a lock. Losers of the race fail the
/// rename with `ENOTEMPTY` (Linux refuses to rename over a non-empty
/// directory) and adopt the winner's tree. A reader therefore only ever sees
/// the final path either absent or complete — never half-written and never
/// being rolled back, which is the exact state `~/.npm/_npx/<hash>` used to be
/// left in. Concurrent cold installs write disjoint staging directories and
/// share only npm's content-addressed `_cacache`, which unlike `_npx` is built
/// for concurrent access.
fn resolve() -> PathBuf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let published = tmp.join(TSX_PIN.replace('@', "-"));
    let cli = cli_path(&published);
    if cli.is_file() {
        return cli;
    }

    let staging = tmp.join(format!(
        ".{}.staging-{}-{}",
        TSX_PIN.replace('@', "-"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos())
    ));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).expect("create tsx staging dir");

    let mut install = Command::new("npm");
    install.args([
        "install",
        "--no-audit",
        "--no-fund",
        "--no-package-lock",
        "--loglevel=error",
        "--prefix",
    ]);
    install.arg(&staging).arg(TSX_PIN);
    let output = install.output().expect("run npm install to resolve tsx");

    let staged_cli = cli_path(&staging);
    assert!(
        output.status.success() && staged_cli.is_file(),
        "failed to resolve {TSX_PIN} for the generated-TypeScript smoke tests\n{}",
        super::command_report(&install, &output)
    );

    match std::fs::rename(&staging, &published) {
        Ok(()) => cli,
        Err(error) => {
            // Another test binary published first; adopt its tree. Anything
            // else is a real failure worth surfacing loudly.
            let _ = std::fs::remove_dir_all(&staging);
            assert!(
                cli.is_file(),
                "publishing {TSX_PIN} to {} failed and no other process had published it: {error}",
                published.display()
            );
            cli
        }
    }
}

fn cli_path(root: &Path) -> PathBuf {
    root.join("node_modules")
        .join("tsx")
        .join("dist")
        .join("cli.mjs")
}
