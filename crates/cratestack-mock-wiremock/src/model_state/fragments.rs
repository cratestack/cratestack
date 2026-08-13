//! Low-level Handlebars text fragments, validated by hand against a
//! real `wiremock/wiremock` + `wiremock-state-extension` container
//! before being encoded here (see `docs/design/wiremock-stubs.md`'s
//! "Model CRUD statefulness" section) — every quoting/escaping choice
//! below exists because the naive version produced a real parse error
//! or a wrong-typed JSON value against the live extension, not because
//! it looked cleaner.

use crate::model_attrs::ScalarKind;

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

/// `{{#if (jsonPath request.body '$.field')}}<new>{{else}}<fallback>{{/if}}`
/// — PATCH semantics for one field: the caller's new value if the patch
/// body included it, otherwise whatever `fallback` renders (typically
/// [`read_state`] against the prior stored value).
pub(crate) fn merge_or_fallback(field: &str, fallback: &str) -> String {
    format!(
        "{{{{#if (jsonPath request.body '$.{field}')}}}}{{{{jsonPath request.body '$.{field}'}}}}{{{{else}}}}{fallback}{{{{/if}}}}"
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
}
