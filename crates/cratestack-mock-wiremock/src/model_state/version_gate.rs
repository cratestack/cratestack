//! When a model declares `@version`, `update`/`delete`'s single
//! `hasContext`-gated stub is replaced by five priority-ordered stubs
//! that mirror the real server's `If-Match` contract exactly
//! (`crates/cratestack-axum/src/headers/etag.rs::parse_if_match_version`,
//! `crates/cratestack-sqlx/src/query/write/update.rs`,
//! `CratestackError::status_code`) — see `docs/design/wiremock-stubs.md`'s
//! "If-Match / optimistic locking" section for the `wiremock-state-
//! extension` capability investigation this is built on. A model with
//! no `@version` field is completely unaffected: [`gated_mappings`] is
//! only ever called from `super::model_state` when
//! `plan.version_name.is_some()`.
//!
//! **Key finding from that investigation:** `wiremock-state-extension`'s
//! `state-matcher` `property` check runs its matcher *value* (e.g. an
//! `equalTo` pattern) through the same `TemplateEngine` response
//! templating uses before comparing it against the stored property —
//! confirmed by reading `StateRequestMatcher.calculateMatch`
//! (`renderTemplateRecursively` is applied to the whole `property` map
//! before `ContextMatcher.property.evaluate` ever runs) and then by
//! hand against the real container. That means a request header can be
//! compared against stored per-record state with no extension change:
//! `{{regexExtract request.headers.If-Match '[0-9]+' ...}}` renders to
//! the header's digits, which `equalTo`/`not equalTo` then compares
//! against the stored `version` property directly. No templated
//! "compare stored state against the request" primitive existed before
//! this was tried — the extension's own README only documents
//! `hasContext`/`hasNotContext` as templated; `property`'s own
//! templating support isn't documented at all, only found by reading
//! the extension's source at the pinned commit.
//!
//! The five stubs, in ascending `priority` (WireMock: lower number wins
//! first) — this ordering matters for exactly one overlap: `If-Match:
//! *` also satisfies case 3's `doesNotMatch`, so case 2 must outrank
//! case 3. The other four pairs are mutually exclusive by header
//! content alone (a header is present or absent, and a present value
//! either matches the strong-ETag regex or doesn't), so priority only
//! has real work to do at that one seam — confirmed by hand, not just
//! reasoned about, against the real container.
//!
//! 1. `If-Match` absent → `412` (native WireMock `"absent": true`).
//! 2. `If-Match: *` → `400` (native WireMock `"equalTo": "*"`).
//! 3. present, not a strong quoted ETag → `400` (native WireMock
//!    `"doesNotMatch"` against [`STRONG_ETAG_PATTERN`]).
//! 4. well-formed but stale (`property` `not equalTo`) → `412`.
//! 5. well-formed and current (`property` `equalTo`) → the real
//!    success response, `success_status`/`success_body`/
//!    `success_listeners` as supplied by the caller.

use serde_json::{Value, json};

use super::fields::ModelFieldPlan;
use super::if_match::{STRONG_ETAG_PATTERN, error_body, if_match_digits};
use super::mapping::{envelope_with_header_matcher, with_etag_header};

#[allow(clippy::too_many_arguments)]
pub(super) fn gated_mappings(
    method: &str,
    url_pattern: &str,
    plan: &ModelFieldPlan,
    success_status: u16,
    success_body: &str,
    success_listeners: Option<Value>,
    // `Some` for `update` (the post-bump `ETag`, `crates/
    // cratestack-axum/src/axum/model/prep/etag.rs`'s
    // `update_etag_apply`), `None` for `delete` (the real server never
    // sets one there — `delete_if_match_apply` has no matching
    // `*_etag_apply` at all).
    success_etag_template: Option<&str>,
    model_name: &str,
    operation: &'static str,
) -> Vec<(String, Value)> {
    let version_name = plan
        .version_name
        .as_deref()
        .expect("gated_mappings is only called for an @version model");
    let has_context = "{{request.path}}";
    let digits = if_match_digits();

    let absent = envelope_with_header_matcher(
        method,
        url_pattern,
        json!({ "If-Match": { "absent": true } }),
        json!({ "hasContext": has_context }),
        412,
        &error_body("PRECONDITION_FAILED", "If-Match header required"),
        None,
        model_name,
        operation,
        1,
    );
    let wildcard = envelope_with_header_matcher(
        method,
        url_pattern,
        json!({ "If-Match": { "equalTo": "*" } }),
        json!({ "hasContext": has_context }),
        400,
        &error_body(
            "BAD_REQUEST",
            "If-Match: * is not supported on versioned models",
        ),
        None,
        model_name,
        operation,
        2,
    );
    let malformed = envelope_with_header_matcher(
        method,
        url_pattern,
        json!({ "If-Match": { "doesNotMatch": STRONG_ETAG_PATTERN } }),
        json!({ "hasContext": has_context }),
        400,
        // The two literal `\"` pairs are JSON-string-escaped double
        // quotes — this is the real message text, `parse_if_match_version`'s
        // own `"If-Match must be a strong ETag of the form \"<integer>\""`.
        &error_body(
            "BAD_REQUEST",
            "If-Match must be a strong ETag of the form \\\"<integer>\\\"",
        ),
        None,
        model_name,
        operation,
        3,
    );
    let stale_message = format!(
        "version mismatch: expected {digits}, found {{{{state context=request.path property='{version_name}'}}}}"
    );
    let stale = envelope_with_header_matcher(
        method,
        url_pattern,
        json!({ "If-Match": { "matches": STRONG_ETAG_PATTERN } }),
        json!({
            "hasContext": has_context,
            "property": { version_name: { "not": { "equalTo": digits } } },
        }),
        412,
        &error_body("PRECONDITION_FAILED", &stale_message),
        None,
        model_name,
        operation,
        4,
    );
    let mut success = envelope_with_header_matcher(
        method,
        url_pattern,
        json!({ "If-Match": { "matches": STRONG_ETAG_PATTERN } }),
        json!({
            "hasContext": has_context,
            "property": { version_name: { "equalTo": digits } },
        }),
        success_status,
        success_body,
        success_listeners,
        model_name,
        operation,
        5,
    );
    if let Some(etag_template) = success_etag_template {
        success = with_etag_header(success, etag_template);
    }

    vec![
        (format!("{operation}-if-match-required"), absent),
        (format!("{operation}-if-match-wildcard"), wildcard),
        (format!("{operation}-if-match-malformed"), malformed),
        (format!("{operation}-if-match-stale"), stale),
        (operation.to_owned(), success),
    ]
}
