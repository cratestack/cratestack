//! `--require-version` / `CRATESTACK_REQUIRE_VERSION` (cratestack#1372):
//! refuse to run when this binary is not the version a script or CI job
//! pinned, so a stale install earlier on `PATH` fails loudly instead of
//! silently generating from the wrong version.

use anyhow::{Result, bail};
use semver::{Version, VersionReq};

pub(crate) const ENV_VAR: &str = "CRATESTACK_REQUIRE_VERSION";

/// Check the process's flag/env against this binary's version.
pub(crate) fn enforce(flag: Option<&str>) -> Result<()> {
    let env = std::env::var(ENV_VAR).ok();
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown executable path>".to_owned());
    check(flag, env.as_deref(), env!("CARGO_PKG_VERSION"), &exe)
}

/// Pure core. The flag wins over the env var; an empty (or all-blank)
/// value counts as unset. A bare version (`0.20.0`) means exactly
/// `=0.20.0`, not Cargo's caret default.
pub(crate) fn check(flag: Option<&str>, env: Option<&str>, running: &str, exe: &str) -> Result<()> {
    let Some(raw) = [flag, env]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
    else {
        return Ok(());
    };
    let req = parse_requirement(raw)?;
    let version = Version::parse(running)
        .map_err(|e| anyhow::anyhow!("running version `{running}` is not valid semver: {e}"))?;
    if req.matches(&version) {
        return Ok(());
    }
    bail!(
        "cratestack {running} does not satisfy the required version `{raw}` \
         (executable: {exe}). Another cratestack install may be earlier on PATH; \
         install the required version or adjust PATH."
    )
}

fn parse_requirement(raw: &str) -> Result<VersionReq> {
    if let Ok(exact) = Version::parse(raw) {
        return Ok(VersionReq {
            comparators: vec![semver::Comparator {
                op: semver::Op::Exact,
                major: exact.major,
                minor: Some(exact.minor),
                patch: Some(exact.patch),
                pre: exact.pre,
            }],
        });
    }
    VersionReq::parse(raw).map_err(|e| anyhow::anyhow!("invalid version requirement `{raw}`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::check;

    fn run(flag: Option<&str>, env: Option<&str>) -> anyhow::Result<()> {
        check(flag, env, "0.20.0", "/opt/bin/cratestack")
    }

    #[test]
    fn bare_version_is_exact() {
        assert!(run(Some("0.20.0"), None).is_ok());
        assert!(run(Some("0.20.1"), None).is_err());
        // Not caret: 0.20.0 must not accept a newer patch, and a running
        // 0.20.1 must not satisfy bare 0.20.0.
        assert!(check(Some("0.20.0"), None, "0.20.1", "x").is_err());
    }

    #[test]
    fn ranges_use_cargo_semantics() {
        assert!(run(Some("=0.20.0"), None).is_ok());
        assert!(run(Some(">=0.19.6, <0.21"), None).is_ok());
        assert!(run(Some(">=0.21"), None).is_err());
        assert!(run(Some("^0.19"), None).is_err());
    }

    #[test]
    fn unsatisfied_message_names_version_requirement_and_exe() {
        let msg = run(Some("0.19.0"), None).unwrap_err().to_string();
        assert!(msg.contains("0.20.0") && msg.contains("0.19.0"));
        assert!(msg.contains("/opt/bin/cratestack") && msg.contains("PATH"));
    }

    #[test]
    fn unparsable_requirement_is_an_error() {
        let msg = run(Some("not a version"), None).unwrap_err().to_string();
        assert!(msg.contains("invalid version requirement"));
    }

    #[test]
    fn flag_wins_over_env() {
        assert!(run(Some("0.20.0"), Some("0.19.0")).is_ok());
        assert!(run(Some("0.19.0"), Some("0.20.0")).is_err());
    }

    #[test]
    fn env_applies_alone_and_empty_is_ignored() {
        assert!(run(None, Some("0.19.0")).is_err());
        assert!(run(None, Some("")).is_ok());
        assert!(run(None, None).is_ok());
        // An empty flag does not mask a set env var.
        assert!(run(Some(""), Some("0.19.0")).is_err());
    }
}
