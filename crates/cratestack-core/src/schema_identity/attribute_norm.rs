//! Whitespace normalisation of `Attribute::raw`, which is verbatim source
//! text: `@default( false )` and `@default(false)` are one attribute.
//!
//! Outside quoted string literals, each whitespace run collapses to one
//! space, and a space next to a punctuation character is dropped. String
//! literal contents (`"…"`, with `\` escapes, and `"""…"""`) stay verbatim,
//! because they carry SQL bodies and regexes where whitespace is meaning.

/// Characters a space beside is never significant.
const PUNCTUATION: &str = "()[]{},:=!<>&|.";

/// Normalises attribute text as described in the module doc.
pub fn normalize_attribute_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut pending_space = false;
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        let is_quote = c == '"';
        if pending_space && !is_punct(out.chars().last()) && !(is_punct(Some(c))) {
            out.push(' ');
        }
        pending_space = false;
        out.push(c);
        if is_quote {
            copy_literal(&mut chars, &mut out);
        }
    }
    out
}

fn is_punct(c: Option<char>) -> bool {
    c.is_some_and(|c| PUNCTUATION.contains(c))
}

/// Copies a string literal's body and closing quote verbatim; the opening
/// quote is already in `out`.
fn copy_literal(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, out: &mut String) {
    let mut lookahead = chars.clone();
    let triple = lookahead.next() == Some('"') && lookahead.next() == Some('"');
    if triple {
        out.push_str("\"\"");
        chars.next();
        chars.next();
        let mut run = 0;
        for c in chars.by_ref() {
            out.push(c);
            run = if c == '"' { run + 1 } else { 0 };
            if run == 3 {
                return;
            }
        }
        return;
    }
    while let Some(c) = chars.next() {
        out.push(c);
        match c {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            }
            '"' => return,
            _ => {}
        }
    }
}
