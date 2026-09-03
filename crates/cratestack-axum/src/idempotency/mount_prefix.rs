//! Stripping a router's mount prefix before descriptor lookup.
//!
//! Generated descriptors record the path the *schema* declares
//! (`/$procs/notify`, `/rpc/procedure.notify`). A consumer who mounts the
//! generated router with `Router::nest("/api", router)` — which is the
//! example this crate's own README gives — serves those ops at
//! `/api/$procs/notify`, and the two no longer compare equal.
//!
//! The consequence was measured and is why this module exists: under a
//! nested mount, *every* lookup missed, every op resolved to
//! [`cratestack_exec::OpAdmission::unresolved`], and `@no_idempotency`
//! silently did nothing. Safe — a miss reserves — but silently inert, which
//! is the exact failure mode #876 set out to end.
//!
//! The prefix is not inferred. `MatchedPath` reports the full path
//! including the nest prefix, but nothing in the request says which leading
//! segments were the mount and which belong to the schema, and guessing
//! would trade a silent no-op for a silent mis-match. The consumer knows
//! its own mount point, so it passes it in.

/// Normalise a caller-supplied mount prefix: `""`, `"/"`, `"/api"` and
/// `"/api/"` all mean the same thing, and all become `""` or `"/api"`.
pub(super) fn normalize(prefix: &str) -> String {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    }
}

/// Remove `prefix` from the front of `path`.
///
/// Returns `None` when `path` is not under `prefix` at a segment boundary,
/// and the caller must treat that as unresolved (i.e. reserve). Requiring
/// the boundary is what stops a `/api` prefix from matching `/apiary/...`
/// and handing back a bogus remainder.
pub(super) fn strip<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() {
        // The mount point itself, not an op under it.
        return Some("/");
    }
    if rest.starts_with('/') {
        Some(rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize, strip};

    #[test]
    fn normalize_accepts_every_spelling_of_the_same_mount() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("/"), "");
        assert_eq!(normalize("/api"), "/api");
        assert_eq!(normalize("/api/"), "/api");
        assert_eq!(normalize("api"), "/api", "a missing leading slash is added");
    }

    #[test]
    fn strip_removes_the_mount_and_keeps_the_schema_path() {
        assert_eq!(strip("/api/$procs/notify", "/api"), Some("/$procs/notify"));
        assert_eq!(strip("/$procs/notify", ""), Some("/$procs/notify"));
        assert_eq!(strip("/api", "/api"), Some("/"));
    }

    #[test]
    fn strip_refuses_a_partial_segment_match() {
        assert_eq!(
            strip("/apiary/$procs/notify", "/api"),
            None,
            "`/api` must not match `/apiary` — returning `ary/$procs/notify` \
             would be a bogus lookup key, and a bogus key that happened to hit \
             would bypass a reservation"
        );
    }

    #[test]
    fn strip_refuses_a_path_outside_the_mount() {
        assert_eq!(strip("/other/$procs/notify", "/api"), None);
    }
}
