//! Does a procedure's policy admit a caller with no session?
//!
//! Decided on the policy's meaning, not its text. The policy is evaluated
//! for an anonymous caller (`auth()` is null) in three values: a term that
//! is certain for such a caller is `True` or `False`; any other term (an
//! argument comparison, a claim) is `Unknown`. This mirrors
//! `cratestack_policy::authorize_procedure`: no `@allow` denies everyone, a
//! `@deny` that is not certainly false refuses, an `@allow` grants only when
//! certainly true, and `@authn` (checked after them) needs a session.

use cratestack_core::Procedure;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Truth {
    True,
    False,
    Unknown,
}

fn and(parts: impl Iterator<Item = Truth>) -> Truth {
    let mut all_true = true;
    for part in parts {
        match part {
            Truth::False => return Truth::False,
            Truth::Unknown => all_true = false,
            Truth::True => {}
        }
    }
    if all_true {
        Truth::True
    } else {
        Truth::Unknown
    }
}

fn or(parts: impl Iterator<Item = Truth>) -> Truth {
    let mut all_false = true;
    for part in parts {
        match part {
            Truth::True => return Truth::True,
            Truth::Unknown => all_false = false,
            Truth::False => {}
        }
    }
    if all_false {
        Truth::False
    } else {
        Truth::Unknown
    }
}

/// One term, for a caller whose `auth()` is null.
fn term(raw: &str) -> Truth {
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    match compact.as_str() {
        "true" | "auth()==null" => Truth::True,
        "false" | "auth()!=null" | "auth().isSystem()" => Truth::False,
        other if other.starts_with("hasRole(") => Truth::False,
        _ => Truth::Unknown,
    }
}

/// Split `input` at top-level (depth 0) occurrences of `op`.
fn split_top<'a>(input: &'a str, op: &str) -> Vec<&'a str> {
    let (mut parts, mut depth, mut start, mut at) = (Vec::new(), 0usize, 0, 0);
    while at < input.len() {
        let rest = &input[at..];
        match rest.as_bytes()[0] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && rest.starts_with(op) => {
                parts.push(&input[start..at]);
                at += op.len();
                start = at;
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    parts.push(&input[start..]);
    parts
}

/// `||` binds loosest, then `&&`, then parentheses; same grammar as the
/// macro that lowers the expression.
fn expression(input: &str) -> Truth {
    let input = input.trim();
    let ors = split_top(input, "||");
    if ors.len() > 1 {
        return or(ors.into_iter().map(expression));
    }
    let ands = split_top(input, "&&");
    if ands.len() > 1 {
        return and(ands.into_iter().map(expression));
    }
    if wrapped(input) {
        return expression(&input[1..input.len() - 1]);
    }
    term(input)
}

/// Whether one pair of parentheses encloses all of `input`.
fn wrapped(input: &str) -> bool {
    if !(input.starts_with('(') && input.ends_with(')')) {
        return false;
    }
    let mut depth = 0usize;
    for (at, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return at == input.len() - 1;
                }
            }
            _ => {}
        }
    }
    false
}

fn clause<'a>(procedure: &'a Procedure, name: &str) -> Vec<&'a str> {
    procedure
        .attributes
        .iter()
        .filter_map(|attribute| {
            attribute
                .raw
                .trim()
                .strip_prefix(name)?
                .trim_start()
                .strip_prefix('(')?
                .trim_end()
                .strip_suffix(')')
        })
        .collect()
}

/// Certainly admits a caller with no session: some `@allow` is certainly
/// true for them, no `@deny` can refuse them, and nothing else needs a
/// session. A procedure with no `@allow` admits no one.
pub(super) fn admits_anonymous(procedure: &Procedure) -> bool {
    let needs_session = procedure.attributes.iter().any(|attribute| {
        let raw = attribute.raw.trim();
        raw.starts_with("@authn") || raw.starts_with("@authorize")
    });
    if needs_session {
        return false;
    }
    let denied = clause(procedure, "@deny")
        .into_iter()
        .any(|raw| expression(raw) != Truth::False);
    !denied
        && clause(procedure, "@allow")
            .into_iter()
            .any(|raw| expression(raw) == Truth::True)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admits(expr: &str) -> bool {
        expression(expr) == Truth::True
    }

    #[test]
    fn terms_for_a_caller_with_no_session() {
        assert!(admits("true"));
        assert!(admits("auth() == null"));
        assert!(!admits("auth() != null"));
        assert!(!admits("false"));
        assert!(!admits("hasRole(\"admin\")"));
        assert!(!admits("some_flag"));
    }

    #[test]
    fn connectives_and_parentheses() {
        assert!(admits("true || auth() != null"));
        assert!(admits("auth() != null || (true && true)"));
        assert!(admits("(true)"));
        assert!(!admits("true && auth() != null"));
        assert!(!admits("(true || x) && auth() != null"));
        assert!(!admits("(auth() != null) || (auth() != null)"));
        assert!(!admits("true && x"));
    }
}
