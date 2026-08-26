//! The literal-cast half of `super::normalize_predicate` — see that
//! function's doc for why it exists and what it deliberately does not
//! attempt. Split into its own file to keep `predicate.rs` under this
//! crate's ~200-LoC convention.

/// Removes a `::<type>` cast Postgres's deparser inserts immediately
/// after a literal — `'active'::text` → `'active'`, `(100)::numeric` →
/// `100` — so a predicate whose only difference from its introspected
/// form is this cast still compares equal.
pub(super) fn strip_literal_casts(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match try_match_cast_literal(&chars, i) {
            Some((literal, next_i)) => {
                out.push_str(&literal);
                i = next_i;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// At `chars[i]`, tries to match `['('] <literal> [')'] '::' <typename>`
/// — a literal, optionally parenthesized (Postgres wraps a numeric
/// constant that needs a cast in one redundant pair, e.g. `(100)::numeric`
/// — a bare string literal never gets this treatment), immediately
/// followed by a cast. `<typename>` is one or more lowercase words
/// separated by single spaces (covers multi-word names like `character
/// varying`/`timestamp without time zone`), optionally followed by a
/// `(...)` type modifier and/or `[]` array markers. Returns the bare
/// literal text and the index just past the whole match, or `None` if
/// `chars[i]` doesn't start this shape — including when it's preceded by
/// an identifier character, which would make `(`/a digit part of a
/// function call or a longer identifier rather than a fresh literal.
fn try_match_cast_literal(chars: &[char], i: usize) -> Option<(String, usize)> {
    if i > 0 && is_identifier_char(chars[i - 1]) {
        return None;
    }
    let mut j = i;
    let has_paren = chars.get(j) == Some(&'(');
    if has_paren {
        j += 1;
    }
    let (literal, mut k) = parse_literal(chars, j)?;
    if has_paren {
        if chars.get(k) != Some(&')') {
            return None;
        }
        k += 1;
    }
    if chars.get(k) != Some(&':') || chars.get(k + 1) != Some(&':') {
        return None;
    }
    k += 2;

    let mut consumed_type = false;
    loop {
        let word_len = parse_lowercase_word(chars, k);
        if word_len == 0 {
            break;
        }
        k += word_len;
        consumed_type = true;
        if chars.get(k) == Some(&'(')
            && let Some(end) = find_matching_paren(chars, k)
        {
            k = end + 1;
        }
        while chars.get(k) == Some(&'[') && chars.get(k + 1) == Some(&']') {
            k += 2;
        }
        match (chars.get(k), chars.get(k + 1)) {
            (Some(' '), Some(next)) if next.is_ascii_lowercase() => k += 1,
            _ => break,
        }
    }
    if !consumed_type {
        return None;
    }
    Some((literal, k))
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Parses a single-quoted string literal (handling `''` as an escaped
/// quote) or a plain numeric literal starting at `chars[j]`. Returns the
/// literal's own text (quotes included, for a string) and the index just
/// past it, or `None` if `chars[j]` doesn't start either shape.
fn parse_literal(chars: &[char], j: usize) -> Option<(String, usize)> {
    match chars.get(j) {
        Some('\'') => {
            let mut literal = String::from("'");
            let mut k = j + 1;
            loop {
                match chars.get(k) {
                    Some('\'') if chars.get(k + 1) == Some(&'\'') => {
                        literal.push_str("''");
                        k += 2;
                    }
                    Some('\'') => {
                        literal.push('\'');
                        return Some((literal, k + 1));
                    }
                    Some(ch) => {
                        literal.push(*ch);
                        k += 1;
                    }
                    None => return None, // unterminated — leave untouched
                }
            }
        }
        Some(ch) if ch.is_ascii_digit() => {
            let mut k = j;
            while matches!(chars.get(k), Some(c) if c.is_ascii_digit()) {
                k += 1;
            }
            if chars.get(k) == Some(&'.')
                && matches!(chars.get(k + 1), Some(c) if c.is_ascii_digit())
            {
                k += 1;
                while matches!(chars.get(k), Some(c) if c.is_ascii_digit()) {
                    k += 1;
                }
            }
            Some((chars[j..k].iter().collect(), k))
        }
        _ => None,
    }
}

/// Length, in chars, of a run of lowercase ASCII letters starting at
/// `chars[k]` — a single word of a (possibly multi-word) type name. `0`
/// means `chars[k]` isn't the start of one, which callers use as the
/// loop-termination signal.
fn parse_lowercase_word(chars: &[char], k: usize) -> usize {
    chars[k..]
        .iter()
        .take_while(|ch| ch.is_ascii_lowercase())
        .count()
}

/// Index of the `)` matching the `(` at `chars[open]`, or `None` if
/// unbalanced.
fn find_matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (offset, ch) in chars[open..].iter().enumerate() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}
