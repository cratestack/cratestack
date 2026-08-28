//! Resolves the pinned `tsx` runner once per target directory, with no
//! `npx` anywhere on the path a test actually takes (cratestack#738).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::publish::{publish_tree, sibling};

/// Pinned, not `tsx@latest`. An unpinned tool inside CI is a dependency whose
/// version changes without a commit here, and the failure it produces lands in
/// a test whose diagnostics are printed only if everything else goes right.
/// 4.23.12 is the latest release as of 2026-08-24.
const TSX_PIN: &str = "tsx@4.23.12";

/// `node` and `npm` — deliberately **not** `npx`, which nothing here invokes
/// any more (cratestack#738). The probe asserts exactly what the harness
/// uses; leaving `npx` in it would be a check on a tool no code path touches.
/// In every environment this guard exists for the three ship together, so the
/// documented skip behaviour — a printed skip where Node is absent, i.e. a
/// local Rust-only checkout rather than CI — is unchanged.
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

/// Install into a private staging directory, then publish it atomically.
///
/// `CARGO_TARGET_TMPDIR` (`<target-dir>/tmp`) is shared by every test binary
/// and is not cleaned between runs, so a published tree is reused across
/// binaries and across invocations of `cargo test`: the steady state locally
/// is one `is_file()` stat and no npm process at all. It lives inside
/// `target/`, so `cargo clean` disposes of it and nothing needs gitignoring.
///
/// On CI that steady state does *not* hold, by design of the cache rather than
/// by accident — `Swatinem/rust-cache` strips every regular file out of this
/// tree before saving it, so each job re-installs once. See `publish.rs` for
/// the mechanism and why the publish path must tolerate it.
fn resolve() -> PathBuf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let published = tmp.join(TSX_PIN.replace('@', "-"));
    if is_complete(&published) {
        return cli_path(&published);
    }

    let staging = sibling(&published, "staging");
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

    assert!(
        output.status.success() && is_complete(&staging),
        "failed to resolve {TSX_PIN} for the generated-TypeScript smoke tests\n{}",
        super::command_report(&install, &output)
    );

    publish_tree(&staging, &published, &is_complete);
    cli_path(&published)
}

/// Validates a candidate tree by the artifact we actually exec, and requires
/// it to be non-empty.
///
/// Mere existence of the directory is not enough and never was: a
/// rust-cache-restored `target/` contains this tree as a skeleton of empty
/// directories with every file unlinked. The length check additionally rejects
/// a truncated restore, where `cli.mjs` exists at zero bytes and would
/// otherwise pass an `is_file()` test only to fail at exec time.
fn is_complete(root: &Path) -> bool {
    std::fs::metadata(cli_path(root)).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

fn cli_path(root: &Path) -> PathBuf {
    root.join("node_modules")
        .join("tsx")
        .join("dist")
        .join("cli.mjs")
}
