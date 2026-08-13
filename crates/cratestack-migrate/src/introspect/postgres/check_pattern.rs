//! Pure-text reconstruction of an enum-membership `CheckKind` from a
//! deparsed CHECK predicate. Split out of [`super::constraints`] (which
//! owns the actual `pg_constraint` query) to keep both files under the
//! repo's file-size convention — this half has no database dependency
//! at all, which is also why its tests run without a live Postgres.

use crate::ir::CheckKind;

/// Recognise `<column> = ANY (ARRAY[...])` / `<column> <@ ARRAY[...]`
/// (after stripping any fully-wrapping redundant parens) and rebuild
/// the [`CheckKind::Enum`] it came from. Returns `None` for anything
/// else — including a multi-column CHECK, already filtered out by the
/// caller's query — which falls back to [`CheckKind::Raw`](crate::ir::CheckKind::Raw).
pub(super) fn reconstruct_enum(column: &str, predicate: &str) -> Option<CheckKind> {
    let normalized = strip_redundant_parens(predicate);

    if let Some(items) = normalized
        .strip_prefix(column)
        .and_then(|s| s.trim_start().strip_prefix("= ANY (ARRAY["))
        .and_then(|s| s.strip_suffix("])"))
    {
        return Some(CheckKind::Enum {
            variants: parse_text_literals(items)?,
            list: false,
        });
    }
    if let Some(items) = normalized
        .strip_prefix(column)
        .and_then(|s| s.trim_start().strip_prefix("<@ ARRAY["))
        .and_then(|s| s.strip_suffix(']'))
    {
        return Some(CheckKind::Enum {
            variants: parse_text_literals(items)?,
            list: true,
        });
    }
    None
}

/// Strips one layer of parens at a time as long as the leading `(`
/// actually matches the trailing `)` (i.e. the string is *fully*
/// wrapped, not just starts-with/ends-with unrelated parens).
fn strip_redundant_parens(mut value: &str) -> &str {
    loop {
        if !value.starts_with('(') || !value.ends_with(')') {
            return value;
        }
        let mut depth = 0i32;
        let mut fully_wrapped = true;
        let bytes = value.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 && i != bytes.len() - 1 {
                        fully_wrapped = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !fully_wrapped {
            return value;
        }
        value = &value[1..value.len() - 1];
    }
}

/// Parses `'a'::text, 'b'::text, ...` into `["a", "b", ...]`. Bails
/// out (`None`) on any item that doesn't match that exact shape —
/// e.g. a numeric or boolean `= ANY (ARRAY[...])`, which is a real SQL
/// pattern but not one an enum CHECK ever produces — rather than guess.
fn parse_text_literals(items: &str) -> Option<Vec<String>> {
    items
        .split(", ")
        .map(|item| {
            let literal = item.strip_suffix("::text")?;
            let inner = literal.strip_prefix('\'')?.strip_suffix('\'')?;
            Some(inner.replace("''", "'"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_scalar_membership_check() {
        // What `pg_get_constraintdef` reports (minus the outer
        // `CHECK (...)` wrapper `super::constraints::introspect_checks`
        // already stripped) for a hand-written
        // `CHECK (status IN ('pending', 'active', 'done'))` — Postgres
        // deparses `IN (...)` as `= ANY (ARRAY[...])`.
        let predicate = "(status = ANY (ARRAY['pending'::text, 'active'::text, 'done'::text]))";
        let kind = reconstruct_enum("status", predicate).expect("should reconstruct");
        assert_eq!(
            kind,
            CheckKind::Enum {
                variants: vec!["pending".into(), "active".into(), "done".into()],
                list: false,
            }
        );
    }

    #[test]
    fn reconstructs_list_containment_check() {
        let predicate = "(statuses <@ ARRAY['a'::text, 'b'::text])";
        let kind = reconstruct_enum("statuses", predicate).expect("should reconstruct");
        assert_eq!(
            kind,
            CheckKind::Enum {
                variants: vec!["a".into(), "b".into()],
                list: true,
            }
        );
    }

    #[test]
    fn non_enum_predicate_falls_back_to_none() {
        let predicate = "((age >= 0) AND (age <= 150))";
        assert_eq!(reconstruct_enum("age", predicate), None);
    }

    #[test]
    fn strip_redundant_parens_normalizes_multiple_layers() {
        assert_eq!(strip_redundant_parens("((a) AND (b))"), "(a) AND (b)");
        assert_eq!(
            strip_redundant_parens("(x = ANY (ARRAY[1]))"),
            "x = ANY (ARRAY[1])"
        );
    }
}
