//! Parses a Postgres type name — the right-hand side of a `::<type>`
//! cast — for the purpose of *comparing* two casts, never for rendering
//! DDL (a cast is always carried through verbatim to the emitted SQL,
//! `emit::postgres::indexes`/`emit::sqlite::indexes`; nothing here ever
//! touches that path).
//!
//! Round 2 review of cratestack#742 (Finding B) is why this exists as a
//! real grammar rather than the ASCII-lowercase-letters-only word run it
//! used to be: that version silently ate anything outside
//! `[a-z]` — digits, underscores, dots, quotes — so `100::int4` lost the
//! trailing `4` off the type name and it landed back on the *literal*
//! (`super::try_match_literal` kept scanning from wherever the type
//! parse stopped), turning the comparable text into `1004`. That's a
//! second, independent false-equality vector on top of Finding A: a
//! hand-written `x = 1004` and a cast comparison `x = 100::int4` would
//! have compared as the identical predicate.
pub(super) fn parse_type_name(chars: &[char], k: usize) -> Option<(String, usize)> {
    let (first, mut pos) = parse_name_segment(chars, k)?;
    let mut out = first;
    // Schema qualification: `NAME '.' NAME ...` (`pg_catalog.int4`,
    // `public.citext`). Kept in the normalized output verbatim — see
    // this module's doc on why an unqualified and a schema-qualified
    // spelling of "the same" type deliberately do NOT compare equal
    // here (no catalog access to confirm they really are the same type,
    // and guessing risks the exact false-equality this module exists to
    // prevent — fails toward churn instead, pinned by
    // `tests::schema_qualified_and_bare_type_names_do_not_match`).
    while chars.get(pos) == Some(&'.') {
        match parse_name_segment(chars, pos + 1) {
            Some((seg, next)) => {
                out.push('.');
                out.push_str(&seg);
                pos = next;
            }
            None => break,
        }
    }
    pos = consume_type_modifier(chars, pos, &mut out);
    // Additional bare, lowercase, space-separated words for multi-word
    // builtin names (`character varying`, `double precision`,
    // `timestamp with time zone`) — these are never quoted or dotted,
    // matching how Postgres actually spells them; a real dotted/quoted
    // segment here would mean this isn't a multi-word builtin continuing
    // but something else entirely, so the loop just stops.
    while let (Some(' '), Some(next)) = (chars.get(pos), chars.get(pos + 1))
        && next.is_ascii_lowercase()
    {
        let word_len = lowercase_word_len(chars, pos + 1);
        out.push(' ');
        out.push_str(
            &chars[pos + 1..pos + 1 + word_len]
                .iter()
                .collect::<String>(),
        );
        pos += 1 + word_len;
        pos = consume_type_modifier(chars, pos, &mut out);
    }
    while chars.get(pos) == Some(&'[') && chars.get(pos + 1) == Some(&']') {
        out.push_str("[]");
        pos += 2;
    }
    Some((out, pos))
}

/// Consumes a `(...)` type modifier (`numeric(10,2)`) immediately at
/// `pos`, if present, appending its raw text (including the parens) to
/// `out` — exact match required on comparison, which is always safe
/// (differing modifiers are a real difference). Returns the index just
/// past it, or `pos` unchanged if there's no modifier there.
fn consume_type_modifier(chars: &[char], pos: usize, out: &mut String) -> usize {
    if chars.get(pos) == Some(&'(')
        && let Some(end) = find_matching_paren(chars, pos)
    {
        out.push_str(&chars[pos..=end].iter().collect::<String>());
        return end + 1;
    }
    pos
}

/// One segment of a dotted type name: a bare identifier — folded to
/// lowercase, since unquoted SQL identifiers are case-insensitive — or a
/// double-quoted identifier (`""` escaping an embedded quote, the same
/// rule `super::parse_literal` applies to single-quoted strings), kept
/// verbatim, since quoting makes a SQL identifier case-*sensitive*.
fn parse_name_segment(chars: &[char], k: usize) -> Option<(String, usize)> {
    match chars.get(k) {
        Some('"') => {
            let mut segment = String::new();
            let mut pos = k + 1;
            loop {
                match chars.get(pos) {
                    Some('"') if chars.get(pos + 1) == Some(&'"') => {
                        segment.push('"');
                        pos += 2;
                    }
                    Some('"') => return Some((segment, pos + 1)),
                    Some(ch) => {
                        segment.push(*ch);
                        pos += 1;
                    }
                    None => return None, // unterminated — leave untouched
                }
            }
        }
        Some(ch) if ch.is_ascii_alphabetic() || *ch == '_' => {
            let mut pos = k;
            while matches!(chars.get(pos), Some(c) if c.is_ascii_alphanumeric() || *c == '_') {
                pos += 1;
            }
            Some((
                chars[k..pos]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
                pos,
            ))
        }
        _ => None,
    }
}

/// Length, in chars, of a run of lowercase ASCII letters starting at
/// `chars[k]` — one word of a multi-word type name's continuation. `0`
/// means `chars[k]` isn't the start of one.
fn lowercase_word_len(chars: &[char], k: usize) -> usize {
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
