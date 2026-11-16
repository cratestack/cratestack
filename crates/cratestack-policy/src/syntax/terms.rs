//! Recognising a single policy term, shared by the macros (which lower it)
//! and the analysis (which classifies it).

/// The spelling of the two whole-term `auth()` null checks. Exact string
/// matches, on purpose: the lowering recognises no other spelling.
pub(crate) const AUTH_NOT_NULL_TERM: &str = "auth() != null";
/// See [`AUTH_NOT_NULL_TERM`].
pub(crate) const AUTH_IS_NULL_TERM: &str = "auth() == null";

/// Recognises `auth().isSystem()` (and the bare `isSystem()` shorthand)
/// with any interior whitespace, e.g. `auth() . isSystem ()`.
///
/// Shared by both policy dialects. It lived in the macros' `policy/model`
/// until cratestack#867's review found that the *procedure* dialect — which
/// a `query`'s `@allow` also uses — had no arm for it at all, so
/// `@allow(auth().isSystem())` died with "policy function `auth` requires
/// a string literal argument". A system-caller reconciliation read is
/// precisely what the `query` construct exists for (epic cratestack#488,
/// webank ADR 0038), so that combination has to work; sharing the
/// recogniser is what keeps the two dialects agreeing on the spelling.
pub(crate) fn is_auth_is_system_term(term: &str) -> bool {
    let squashed = term
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    squashed == "auth().isSystem()" || squashed == "isSystem()"
}

/// The contents of a `"…"` or `'…'` literal, or `None`.
pub fn parse_string_literal(value: &str) -> Option<&str> {
    if let Some(value) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return Some(value);
    }
    value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
}

/// `name("literal")` as `(name, literal)`; `None` when the term is not a
/// single call shape, `Some(Err)` when it is one with a bad argument.
pub(crate) fn parse_builtin_policy_call(term: &str) -> Option<Result<(&str, &str), String>> {
    let open = term.find('(')?;
    let name = term[..open].trim();
    let args = term[open + 1..].strip_suffix(')')?.trim();
    if name.is_empty() {
        return Some(Err(format!("invalid policy function `{term}`")));
    }
    if args.is_empty() {
        return Some(Err(format!(
            "policy function `{name}` requires a single string literal argument"
        )));
    }
    if args.contains(',') {
        return Some(Err(format!(
            "policy function `{name}` currently supports exactly one string literal argument"
        )));
    }
    let Some(value) = parse_string_literal(args) else {
        return Some(Err(format!(
            "policy function `{name}` requires a string literal argument"
        )));
    };
    Some(Ok((name, value)))
}
