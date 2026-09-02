//! Spans of a SQL body that Postgres reads as *text*, not as syntax —
//! string literals, dollar-quoted bodies, quoted identifiers and comments
//! — so [`super::scan_sql_placeholders`] can step over them.
//!
//! **Why this exists, and why it is not "parsing SQL".** The first version
//! of the scanner matched `$` + digits anywhere in the body and claimed in
//! its own doc that dollar-quoting was "skipped naturally". That claim was
//! false, and cratestack#867's review measured it: `$$1$$` yielded `[1]`
//! and `$q$99$q$` yielded `[99]`. The consequence is worse than a missed
//! check — it makes the validator **falsely reject valid SQL**. A body
//! containing `WHERE note = $$costs $99$$` with one declared parameter
//! reports "references parameter `$99`, but only 1 parameter(s) are
//! declared" and refuses to compile a schema that is entirely correct.
//!
//! Recognising where a literal starts and ends is lexing, not parsing: it
//! needs no grammar, no expression tree and no catalogue. That is the same
//! line design §2/§3 draws when it rejects extracting *types* from the SQL
//! — the cost there is a type checker that tracks every Postgres cast and
//! function; the cost here is the five rules below, which are fixed by the
//! lexical structure of the language and do not grow.

/// Where a text span begins, if one begins at `bytes[index]`, paired with
/// the index just past its end.
///
/// Returns `None` when nothing starts here — including for `$1`, which
/// must *not* be mistaken for a dollar-quote opener.
pub(super) fn text_span_end(bytes: &[u8], index: usize) -> Option<usize> {
    match bytes[index] {
        // Standard string literal. `''` is an escaped quote, not a
        // terminator — `'it''s $1'` is one literal, not two.
        b'\'' => Some(closing_quote(bytes, index, b'\'')),
        // Quoted identifier. Same doubling rule, and it can contain `$`:
        // `SELECT x AS "cost $1"`.
        b'"' => Some(closing_quote(bytes, index, b'"')),
        b'$' => dollar_quote_end(bytes, index),
        b'-' if bytes.get(index + 1) == Some(&b'-') => Some(line_comment_end(bytes, index)),
        b'/' if bytes.get(index + 1) == Some(&b'*') => Some(block_comment_end(bytes, index)),
        _ => None,
    }
}

/// End of a `'…'` / `"…"` span, treating a doubled quote as an escape.
/// An unterminated literal consumes the rest of the body — which is the
/// fail-closed choice: a malformed body is Postgres's error to report,
/// and scanning its tail for `$N` could only invent an error of our own.
fn closing_quote(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        if bytes[cursor] != quote {
            cursor += 1;
            continue;
        }
        if bytes.get(cursor + 1) == Some(&quote) {
            cursor += 2;
            continue;
        }
        return cursor + 1;
    }
    bytes.len()
}

/// End of a `$tag$…$tag$` span, or `None` if `bytes[start]` does not open
/// one.
///
/// A tag is empty (`$$`) or a Postgres identifier that does **not** start
/// with a digit — which is precisely what keeps `$1` out: its "tag" would
/// begin with `1`, so it is a parameter, not a delimiter. That single rule
/// is why parameters and dollar quotes can share a sigil at all.
fn dollar_quote_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start + 1;
    while cursor < bytes.len() && bytes[cursor] != b'$' {
        let byte = bytes[cursor];
        let valid = byte == b'_' || byte.is_ascii_alphabetic() || {
            // Digits are legal inside a tag, just not as its first byte.
            byte.is_ascii_digit() && cursor > start + 1
        };
        if !valid {
            return None;
        }
        cursor += 1;
    }
    if cursor >= bytes.len() {
        // Ran off the end without a closing `$`: not a delimiter.
        return None;
    }
    let delimiter = &bytes[start..=cursor];
    let body_start = cursor + 1;
    let mut probe = body_start;
    while probe + delimiter.len() <= bytes.len() {
        if &bytes[probe..probe + delimiter.len()] == delimiter {
            return Some(probe + delimiter.len());
        }
        probe += 1;
    }
    // Opened but never closed — consume the rest, same fail-closed
    // reasoning as `closing_quote`.
    Some(bytes.len())
}

/// End of a `-- …` comment: the newline, or the end of the body.
fn line_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start + 2;
    while cursor < bytes.len() && bytes[cursor] != b'\n' {
        cursor += 1;
    }
    cursor
}

/// End of a `/* … */` comment. Postgres block comments **nest**, unlike
/// C's, so this counts depth rather than stopping at the first `*/` —
/// otherwise `/* outer /* inner */ WHERE x = $9 */` would leave the tail
/// of the comment being scanned as live SQL.
fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut cursor = start + 2;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
            depth += 1;
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return cursor;
            }
            continue;
        }
        cursor += 1;
    }
    bytes.len()
}
