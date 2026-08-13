//! Assembles one WireMock mapping JSON document from an already-built
//! request-match shape, response body template, and (for write verbs)
//! `serveEventListeners` array. Response templating (`"transformers":
//! ["response-template"]`) is declared on every stub so generated
//! stubs work whether or not the WireMock instance was also started
//! with `--global-response-templating`.

use serde_json::{Value, json};

/// A `list`/`create` mapping — no `customMatcher`, an exact `urlPath`.
pub(super) fn envelope(
    method: &str,
    url_path: &str,
    status: u16,
    body: &str,
    listeners: Option<Value>,
    model_name: &str,
    operation: &'static str,
) -> Value {
    build(
        json!({ "method": method, "urlPath": url_path }),
        status,
        body,
        listeners,
        model_name,
        operation,
    )
}

/// A `get`/`update`/`delete` mapping — `urlPathPattern` (any id) gated
/// by a `state-matcher` `hasContext` check, so a request for a record
/// that was never created (or was already deleted) falls through to no
/// stub matching at all, i.e. WireMock's own 404 — not something this
/// generator has to emit itself. `priority: 1` leaves headroom below
/// for a consumer to layer hand-authored overrides on top.
pub(super) fn envelope_with_matcher(
    method: &str,
    url_pattern: &str,
    has_context: &str,
    status: u16,
    body: &str,
    listeners: Option<Value>,
    model_name: &str,
    operation: &'static str,
) -> Value {
    let request = json!({
        "method": method,
        "urlPathPattern": url_pattern,
        "customMatcher": { "name": "state-matcher", "parameters": { "hasContext": has_context } },
    });
    let mut mapping = build(request, status, body, listeners, model_name, operation);
    mapping["priority"] = json!(1);
    mapping
}

/// One of the five `If-Match` sub-cases (`super::version_gate`) for a
/// `PATCH`/`DELETE` route on an `@version` model: the same
/// `state-matcher` `hasContext` gate as [`envelope_with_matcher`], plus
/// native WireMock header matching and (optionally) a `state-matcher`
/// `property` comparison, at an explicit `priority` so the five stay
/// resolvable relative to each other regardless of declaration order
/// (verified by hand against the real extension — see
/// `docs/design/wiremock-stubs.md`'s "If-Match / optimistic locking"
/// section).
pub(super) fn envelope_with_header_matcher(
    method: &str,
    url_pattern: &str,
    headers: Value,
    matcher_params: Value,
    status: u16,
    body: &str,
    listeners: Option<Value>,
    model_name: &str,
    operation: &'static str,
    priority: u16,
) -> Value {
    let request = json!({
        "method": method,
        "urlPathPattern": url_pattern,
        "headers": headers,
        "customMatcher": { "name": "state-matcher", "parameters": matcher_params },
    });
    let mut mapping = build(request, status, body, listeners, model_name, operation);
    mapping["priority"] = json!(priority);
    mapping
}

/// Adds a quoted-integer `ETag` response header (`crates/
/// cratestack-axum/src/headers/etag.rs::set_version_etag`'s exact
/// format) around `version_template`, an already-fully-bracketed
/// Handlebars expression (e.g. the output of `super::fragments::
/// read_state`/`version_bump`) — used for `get`'s current-version
/// header and `update`'s post-bump one; never `delete` (the real server
/// never sets one there either — deleting a record leaves no version to
/// advertise).
pub(super) fn with_etag_header(mut mapping: Value, version_template: &str) -> Value {
    mapping["response"]["headers"]["ETag"] = json!(format!("\"{version_template}\""));
    mapping
}

fn build(
    request: Value,
    status: u16,
    body: &str,
    listeners: Option<Value>,
    model_name: &str,
    operation: &'static str,
) -> Value {
    let mut mapping = json!({
        "request": request,
        "response": {
            "status": status,
            "headers": { "Content-Type": "application/json" },
            "body": body,
            "transformers": ["response-template"],
        },
        "metadata": {
            "cratestack": {
                "generated": true,
                "stateful": true,
                "model": model_name,
                "operation": operation,
            },
        },
    });
    if let Some(listeners) = listeners {
        mapping["serveEventListeners"] = listeners;
    }
    mapping
}

/// Escapes Java-regex metacharacters so a `--base-path` containing one
/// is matched literally in a `urlPathPattern`, not interpreted as regex
/// syntax. `model_route_segment`'s own output is always
/// `[a-zA-Z0-9_]+`-shaped, so this only ever has real work to do on
/// `base`.
pub(super) fn regex_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(
            ch,
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::regex_escape;

    #[test]
    fn escapes_regex_metacharacters() {
        assert_eq!(regex_escape("/api.v2"), "/api\\.v2");
        assert_eq!(regex_escape("/api"), "/api");
    }
}
