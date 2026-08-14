//! Small Handlebars/JSON primitives [`super::version_gate::gated_mappings`]
//! assembles into its five stubs — split out to keep that file under
//! this crate's ~200-LoC convention, not because these are reusable
//! elsewhere.

/// A strong ETag payload: literal double quotes around an optionally
/// signed integer — mirrors `parse_if_match_version`'s
/// (`crates/cratestack-axum/src/headers/etag.rs`)
/// `strip_prefix('"')`/`strip_suffix('"')` + `str::parse::<i64>()`
/// shape exactly. A leading `-` is accepted at this shape-check level
/// even though a real stored version is never negative: an
/// out-of-range negative `If-Match` simply can never equal a stored
/// value, so it falls through to the "stale" case, same as the real
/// server would via a doomed-to-fail SQL `WHERE version = -1`.
pub(super) const STRONG_ETAG_PATTERN: &str = "^\"-?[0-9]+\"$";

/// `{{regexExtract request.headers.If-Match '[0-9]+' default=...}}` —
/// the bare digits of a well-formed `If-Match` header, in the same
/// shape a stored `version` property is kept in (see `super::body`'s
/// module doc: never quoted internally, only at the HTTP `ETag`/
/// `If-Match` boundary). The `default=` value is one no real version
/// will ever equal, so a request that somehow reaches this matcher
/// without a well-formed header (shouldn't happen given
/// `gated_mappings`'s priority ordering) never accidentally matches —
/// defense in depth, not a case this generator expects to be reachable.
pub(super) fn if_match_digits() -> String {
    "{{regexExtract request.headers.If-Match '[0-9]+' default='__cratestack_if_match_no_digits__'}}"
        .to_owned()
}

/// A `CratestackErrorResponse`-shaped body (`cratestack_core::
/// CratestackErrorResponse`; `crates/cratestack-axum/src/transport/
/// http_transport.rs` serializes exactly this shape for a REST JSON
/// error, no extra wrapper) — `message` may itself contain Handlebars
/// expressions (the "stale" case's message does), so every stub built
/// from this always declares `"transformers": ["response-template"]`
/// the same as every other generated stub, whether or not `message`
/// uses that.
pub(super) fn error_body(code: &str, message: &str) -> String {
    format!("{{ \"code\": \"{code}\", \"message\": \"{message}\", \"details\": null }}")
}
