//! `tracing_subscriber` bootstrap.
//!
//! Absorbed from a downstream project's `telemetry-kit` (49 lines wrapping
//! `tracing_subscriber`) as part of this crate rather than as its own
//! crate: on its own it is a handful of lines with no framework-shaped
//! concept behind it — every `main()` needs *a* subscriber installed
//! before the first log line, and that need has nothing to do with a
//! schema, a facade, or a layer. It earns a place here because
//! [`crate::run`] and the health checks already live in a "batteries for
//! `main()`" crate; it would not have earned a *standalone* crate.

use tracing_subscriber::EnvFilter;

/// Default log directive when `RUST_LOG` is unset: `info` everywhere,
/// except `tower_http=warn` so the per-request access spans that
/// [`crate::run`] installs (which fire on every `/healthz` probe) don't
/// drown the rest of the log. Set `RUST_LOG=info,tower_http=info` to see
/// request logs when debugging traffic.
pub const DEFAULT_FILTER: &str = "info,tower_http=warn";

/// Install the global `tracing` subscriber.
///
/// - Honours `RUST_LOG` when set (standard [`EnvFilter`] syntax) — this
///   variable name is a Rust-ecosystem-wide convention, not
///   service-specific, so it is deliberately *not* prefixed like every
///   other variable this crate reads.
/// - ANSI colour is always off: these logs are read through `kubectl`
///   or a log aggregator, where escape codes are noise, never a TTY.
/// - Emits plain text by default, or line-delimited JSON when
///   `{prefix}_LOG_FORMAT=json` is set (for ingestion into an
///   ELK/Loki-shaped pipeline).
///
/// Idempotent: uses `try_init`, so a second call (e.g. a test harness that
/// also installs its own subscriber) is a no-op rather than a panic.
pub fn init(prefix: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);

    if is_json_format(std::env::var(log_format_var(prefix)).ok().as_deref()) {
        let _ = builder.json().try_init();
    } else {
        let _ = builder.try_init();
    }
}

fn log_format_var(prefix: &str) -> String {
    format!("{prefix}_LOG_FORMAT")
}

/// Pure parsing logic split out from [`init`] so it is unit-testable
/// without touching process environment (this workspace `forbid`s
/// `unsafe_code`, and mutating env vars from a test is unsound against
/// concurrent reads on other threads without it).
fn is_json_format(value: Option<&str>) -> bool {
    value
        .map(|value| value.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{is_json_format, log_format_var};

    #[test]
    fn recognizes_json_case_insensitively() {
        assert!(is_json_format(Some("json")));
        assert!(is_json_format(Some("JSON")));
        assert!(is_json_format(Some("Json")));
    }

    #[test]
    fn defaults_to_plain_text() {
        assert!(!is_json_format(None));
        assert!(!is_json_format(Some("")));
        assert!(!is_json_format(Some("text")));
        assert!(!is_json_format(Some("pretty")));
    }

    #[test]
    fn log_format_var_is_prefixed() {
        assert_eq!(log_format_var("AUTH"), "AUTH_LOG_FORMAT");
        assert_eq!(log_format_var("CATALOG"), "CATALOG_LOG_FORMAT");
    }
}
