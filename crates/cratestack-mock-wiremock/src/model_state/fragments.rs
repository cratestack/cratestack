//! Low-level Handlebars text fragments, validated by hand against a
//! real `wiremock/wiremock` + `wiremock-state-extension` container
//! before being encoded here (see `docs/design/wiremock-stubs.md`'s
//! "Model CRUD statefulness" section) — every quoting/escaping choice
//! below exists because the naive version produced a real parse error
//! or a wrong-typed JSON value against the live extension, not because
//! it looked cleaner.

use crate::model_attrs::ScalarKind;

/// The JSON key a *list* entry (never a per-record context) stores its
/// own per-record context path under — see `super::listeners`'
/// module doc for why. Not derived from any schema field name, so it
/// can never collide with a real model field (`.cstack` field names are
/// plain identifiers; this key's `__cratestack_` prefix and non-
/// identifier-adjacent shape make an accidental collision practically
/// impossible, and even a colliding field would only ever affect the
/// mock's own bookkeeping, never a real deployment).
pub(crate) const LIST_ENTRY_CONTEXT_KEY: &str = "__cratestack_record_context";

/// Wraps `expr` (a bare Handlebars expression, e.g. `{{state ...}}`) in
/// literal JSON quotes if `kind` needs them (`QuotedString`), or leaves
/// it bare for `Number`/`Bool` so the rendered text is an unquoted JSON
/// number/boolean — `wiremock-state-extension`'s state store has no
/// concept of field types, only what got stored; quoting is entirely
/// this generator's responsibility at render time (verified empirically:
/// storing a number's digits as a string internally and rendering them
/// unquoted still produces valid JSON, since this is text interpolation,
/// not typed serialization).
pub(crate) fn quote_wrap(kind: ScalarKind, expr: &str) -> String {
    match kind {
        ScalarKind::QuotedString => format!("\"{expr}\""),
        ScalarKind::Number | ScalarKind::Bool | ScalarKind::Unsupported => expr.to_owned(),
    }
}

/// `{{jsonPath response.body '$.field'}}` — this same response's
/// already-rendered value for `field`, used by `serveEventListeners` so
/// persisting a record never recomputes (and risks diverging from) the
/// conditional logic that produced the response body — including
/// `{{randomValue}}` for a generated id, which would silently generate
/// a SECOND, different value if re-evaluated instead of harvested from
/// `response.body`.
pub(crate) fn echo_from_response(field: &str) -> String {
    format!("{{{{jsonPath response.body '$.{field}'}}}}")
}

/// `{{state context=<context_expr> property='field'}}` — the stored
/// value for `field` in the per-record context `context_expr` (already
/// a Handlebars expression, e.g. `request.path`, not a string literal).
pub(crate) fn read_state(field: &str, context_expr: &str) -> String {
    format!("{{{{state context={context_expr} property='{field}'}}}}")
}

/// `{{math (state context=<context_expr> property='field') '+' 1}}` —
/// `field`'s stored value in `context_expr`, plus one. Used only for an
/// `@version` field's update-time bump (`super::body::update_body`,
/// `super::version_gate`'s success-case `ETag`): WireMock's bundled
/// `math` helper (jknack `NumberHelpers`, confirmed present in the real
/// extension image — it's part of the same `response-template`
/// transformer, not something `wiremock-state-extension` adds itself)
/// accepts a `{{state ...}}` sub-expression directly as its left-hand
/// operand, no intermediate variable needed. Confirmed by hand: a fresh
/// record's `version` starts at `0` (`create_body`'s literal seed,
/// mirroring `create_exec.rs`'s server-side seed), and one successful
/// `PATCH` renders/stores `1` — matching `update_exec.rs`'s
/// `version = version + 1` exactly.
pub(crate) fn version_bump(field: &str, context_expr: &str) -> String {
    format!("{{{{math (state context={context_expr} property='{field}') '+' 1}}}}")
}

/// A value no real request field is expected to send — used by
/// [`merge_or_fallback`] to tell "key absent from the request body" apart
/// from "key present with a falsy value" (see that function's doc
/// comment). Not a security boundary (this is a mock's mutable request
/// body, not an access-controlled input), so a distinctive plain string
/// is enough; it doesn't need to be cryptographically unguessable.
const ABSENT_SENTINEL: &str = "__cratestack_wiremock_absent__";

/// `{{#if (eq (jsonPath request.body '$.field' default=SENTINEL)
/// SENTINEL)}}<fallback>{{else}}<new>{{/if}}` — PATCH semantics for one
/// field: the caller's new value if the patch body included it,
/// otherwise whatever `fallback` renders (typically [`read_state`]
/// against the prior stored value).
///
/// This presence-tests the field instead of testing its truthiness.
/// The naive `{{#if (jsonPath request.body '$.field')}}` this replaces
/// treats Handlebars/JSON falsy values (`false`, `0`, `""`) the same as
/// "absent" — confirmed by hand against the real extension: `PATCH
/// {"count":0}` on a stored `count:5` left `5` untouched, `{"active":
/// false}` left `true` untouched, and `{"name":""}` left the prior name
/// untouched. A mock consumer could never zero a counter, clear a
/// string, or toggle a boolean off.
///
/// The fix: `jsonPath ... default=SENTINEL` returns the field's real
/// value when the key is present with a non-null value (whatever it is,
/// including `0`/`false`/`""`), and the sentinel both when the key is
/// missing *and* when it's present but explicitly `null` (confirmed by
/// hand — the extension's `jsonPath` helper treats a JSON `null` leaf
/// the same as a missing path for `default=` purposes, it does not
/// return a literal `null`); `eq` (bundled in handlebars.java's
/// `ConditionalHelpers`, confirmed available in the real extension
/// image) compares the two tri-state-safely regardless of the returned
/// value's JSON type — a number/boolean/string all compare `false`
/// against a string sentinel without erroring, confirmed by hand.
///
/// **Explicit JSON `null` is therefore treated the same as "absent"
/// (falls back), not stored as a literal `null`.** This falls out of
/// the `jsonPath`/`default=` behavior above without extra code, and it's
/// also the semantically right call: this helper only ever wraps a
/// stateful field, and every stateful field
/// ([`crate::model_attrs::ScalarKind`]) is `Required` arity by
/// definition — `Optional`/nullable fields are never stateful, they're
/// frozen (see `crate::model_state::fields`). A `Required` field has no
/// valid `null` state to move into, so treating a client's explicit
/// `null` as "leave it alone" is the least-surprising behavior for a
/// mock: the alternative (storing a literal `null` into a field the
/// schema declares non-nullable) would make a later `get` return a
/// shape the real server's type could never produce. Confirmed by hand:
/// `{"count":null}` and `{}` render identically.
pub(crate) fn merge_or_fallback(field: &str, fallback: &str) -> String {
    format!(
        "{{{{#if (eq (jsonPath request.body '$.{field}' default='{ABSENT_SENTINEL}') \
         '{ABSENT_SENTINEL}')}}}}{fallback}{{{{else}}}}{{{{jsonPath request.body '$.{field}'}}}}{{{{/if}}}}"
    )
}

/// The create-time id-generation expression for a primary key of
/// `kind`, unwrapped (the caller applies [`quote_wrap`]). `Cuid`/`Uuid`
/// aren't in [`ScalarKind`] (both classify as `QuotedString`, same as
/// plain `String`) — this takes the PK field's actual type name
/// separately so `Cuid`/`Uuid` still get a plausible-looking generated
/// id instead of a generic random string.
pub(crate) fn id_generator(pk_type_name: &str, kind: ScalarKind) -> String {
    match (pk_type_name, kind) {
        // A bare `{{randomValue length=6 type='NUMERIC'}}` can start
        // with `0` (confirmed by hand: roughly 1 in 10 generated ids),
        // which — rendered unquoted, as every `Number`-kind field is —
        // produces a leading-zero JSON number (`084839`), invalid per
        // the JSON spec and rejected by a strict parser (`serde_json`,
        // `JSON.parse`, ...) on roughly 1 in 10 `create` calls. Fixing
        // the first digit to a literal non-zero `1` guarantees valid
        // JSON on every call, at the cost of the id no longer being
        // uniformly random across the full digit space — irrelevant for
        // a mock, where uniqueness (not distribution) is what matters.
        (_, ScalarKind::Number) => "1{{randomValue length=5 type='NUMERIC'}}".to_owned(),
        ("Uuid", _) => "{{randomValue type='UUID'}}".to_owned(),
        ("Cuid", _) => "c{{randomValue length=24 type='ALPHANUMERIC' uppercase=false}}".to_owned(),
        // Plain `String` PK, or any other/unexpected kind (defense in
        // depth — validated schemas always give a PK one of the kinds
        // above) — an opaque random string is the same fallback
        // `values.rs` static defaults use for "no better shape known".
        _ => "{{randomValue length=16 type='ALPHANUMERIC'}}".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_wrap_only_quotes_string_kind() {
        assert_eq!(quote_wrap(ScalarKind::QuotedString, "X"), "\"X\"");
        assert_eq!(quote_wrap(ScalarKind::Number, "X"), "X");
        assert_eq!(quote_wrap(ScalarKind::Bool, "X"), "X");
    }

    /// Presence-tested (`eq (jsonPath ... default=SENTINEL) SENTINEL`),
    /// never a bare truthiness `#if` on the field's own value — a bare
    /// `#if` is exactly the cratestack#588 falsy-value bug this replaced
    /// (see the function's own doc comment for the real-request repro).
    #[test]
    fn merge_or_fallback_presence_tests_not_truthiness_tests() {
        let expr = merge_or_fallback("count", "FALLBACK");
        assert!(
            expr.contains("(eq (jsonPath request.body '$.count' default="),
            "must presence-test via `eq`+`default=`, not a bare `#if`: {expr}"
        );
        assert!(
            !expr.starts_with("{{#if (jsonPath request.body '$.count')}}"),
            "must not regress to the naive truthiness-testing form: {expr}"
        );
        assert!(
            expr.contains("FALLBACK"),
            "fallback branch must survive: {expr}"
        );
        assert!(
            expr.contains("{{jsonPath request.body '$.count'}}"),
            "the present-value branch must still echo the real request value: {expr}"
        );
    }
}
