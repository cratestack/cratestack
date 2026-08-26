//! Tokenizes a predicate into literal-vs-everything-else [`Segment`]s so
//! `super::predicates_equivalent` can compare two predicates' literals
//! (and, where present, their explicit `::type` casts) *jointly* rather
//! than independently normalizing each side into a plain string first.
//!
//! Round 2 review of cratestack#742 (Finding A) is why this isn't a
//! plain string-rewriting `strip_literal_casts` anymore: independently
//! stripping every `::type` cast from both sides before comparing them
//! made two predicates that cast the *same* literal to two *different*
//! types compare equal — `amount > '100'::int` vs. `amount > '100'::text`,
//! or the money-relevant case, `email = 'x'::citext` (case-insensitive)
//! vs. `email = 'x'::text` (case-sensitive) — because the type name was
//! captured, verified to be present, and then thrown away. Tokenizing
//! into segments keeps each literal's cast type (if any) around so the
//! caller can require a match between two *explicit* casts while still
//! forgiving the common case of one side lacking a cast entirely (the
//! `pg_get_expr`-inserted-it-but-the-author-didn't-write-it case this
//! module was built for in the first place).

mod type_name;

/// One chunk of a tokenized predicate, in original order.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Segment {
    /// A literal's own text (quotes included, for a string), plus its
    /// cast type if one immediately follows (`::<type>`, normalized —
    /// see [`type_name::parse_type_name`]). `None` means the literal
    /// appeared bare, either because the author wrote no cast or
    /// because Postgres didn't need to insert one.
    Literal {
        text: String,
        cast_type: Option<String>,
    },
    /// A run of everything else — operators, keywords, identifiers,
    /// whitespace (already collapsed by the caller), parentheses that
    /// aren't part of a literal-cast wrapper.
    Other(String),
}

/// Splits `input` into [`Segment`]s in order. Every literal in `input`
/// becomes its own `Literal` segment (whether or not a cast follows —
/// alignment between two sides depends on both recognizing the same
/// literal boundaries); everything between them collapses into `Other`
/// runs.
pub(super) fn tokenize(input: &str) -> Vec<Segment> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut other = String::new();
    let mut i = 0;
    while i < chars.len() {
        match try_match_literal(&chars, i) {
            Some((text, cast_type, next_i)) => {
                if !other.is_empty() {
                    out.push(Segment::Other(std::mem::take(&mut other)));
                }
                out.push(Segment::Literal { text, cast_type });
                i = next_i;
            }
            None => {
                other.push(chars[i]);
                i += 1;
            }
        }
    }
    if !other.is_empty() {
        out.push(Segment::Other(other));
    }
    out
}

/// At `chars[i]`, tries to match a literal, optionally parenthesized
/// (Postgres wraps a numeric constant that needs a cast in one redundant
/// pair, e.g. `(100)::numeric` — a bare string literal never gets this
/// treatment) — the parenthesized form is *only* recognized when a cast
/// actually follows, so `(100)` with no cast is left as plain `Other`
/// text (a `(`, a bare literal, a `)`) rather than misread — optionally
/// followed by `'::' <typename>` (see [`type_name::parse_type_name`] for
/// the grammar). Returns the literal's own text, its cast type if one
/// was recognized, and the index just past the whole match; `None` if
/// `chars[i]` doesn't start a literal at all, *or* if `::` is present
/// but what follows isn't a type name this module's grammar recognizes
/// — deliberately not falling back to "bare literal, ignore the `::…`"
/// in that case, since silently dropping an unrecognized cast is exactly
/// the false-equality failure mode this module exists to avoid; the
/// caller's tokenizer just walks into it a character at a time instead,
/// which fails toward two predicates comparing as structurally different
/// (churn) rather than silently equal.
fn try_match_literal(chars: &[char], i: usize) -> Option<(String, Option<String>, usize)> {
    if i > 0 && is_identifier_char(chars[i - 1]) {
        return None;
    }
    if chars.get(i) == Some(&'(') {
        let (literal, k1) = parse_literal(chars, i + 1)?;
        if chars.get(k1) != Some(&')') {
            return None;
        }
        let k2 = k1 + 1;
        if chars.get(k2) != Some(&':') || chars.get(k2 + 1) != Some(&':') {
            return None;
        }
        let (cast_type, k3) = type_name::parse_type_name(chars, k2 + 2)?;
        return Some((literal, Some(cast_type), k3));
    }
    let (literal, k1) = parse_literal(chars, i)?;
    if chars.get(k1) == Some(&':') && chars.get(k1 + 1) == Some(&':') {
        let (cast_type, k2) = type_name::parse_type_name(chars, k1 + 2)?;
        return Some((literal, Some(cast_type), k2));
    }
    Some((literal, None, k1))
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
