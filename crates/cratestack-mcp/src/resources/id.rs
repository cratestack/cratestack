//! A record URI's last segment, as the id the generated code reads.
//!
//! The id is one RFC 3986 path segment, `segment = *pchar` (§ 3.3), so that
//! is the set its raw characters must come from (maintainer decision on
//! #1040): `pchar = unreserved / pct-encoded / sub-delims / ":" / "@"`.
//! Why the path-segment set and not all of RFC 3986's URI characters: the
//! other characters a URI may carry raw are the `gen-delims` `/ ? # [ ]`,
//! and none of them can be part of this segment. `/` would start another
//! segment, `?` the query and `#` a fragment, which `uri.rs` has already
//! split on or refused; `[` and `]` are allowed only in an IP-literal host
//! (§ 3.2.2). So `pchar` is exactly what may appear here, and anything else
//! raw (a space, a control, `"`, `<`, `>`, `\`, `^`, `` ` ``, `{`, `|`, `}`,
//! any non-ASCII character) makes the text not a URI at all.
//!
//! Such an id is "not found", the same answer as a missing row, matching
//! the rest of the matcher's strictness: accepting `a b` raw as well as
//! `a%20b` would give one record two spellings, one of which no conforming
//! client produces. Its percent-encoded form is the character, as always.

/// The decoded id, or `None` when `raw` addresses no record: a raw
/// character outside `pchar`, a malformed escape, bytes that are not UTF-8,
/// an empty id, or a NUL.
pub(super) fn record_id(raw: &str) -> Option<String> {
    if !raw.bytes().all(is_pchar_or_percent) {
        return None;
    }
    let id = percent_decode(raw)?;
    // A NUL is in no key: `Int`/`Uuid` never parse one, and Postgres
    // refuses it in `text` with an error rather than matching nothing — a
    // `-32603` and a server-side error log any caller could trigger at
    // will, where the true answer is "no such row". Deliberately unlike
    // REST, which 500s.
    if id.is_empty() || id.contains('\0') {
        return None;
    }
    Some(id)
}

/// A byte `pchar` allows raw, or the `%` that starts a `pct-encoded`
/// triplet (whose two hex digits [`percent_decode`] checks). Every byte of
/// a non-ASCII character is `>= 0x80`, so none passes.
fn is_pchar_or_percent(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            // unreserved, beyond ALPHA / DIGIT
            b'-' | b'.' | b'_' | b'~'
            // sub-delims
            | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
            // the two pchar adds, and the escape
            | b':' | b'@' | b'%'
        )
}

/// RFC 3986 percent-decoding into UTF-8. `None` for a malformed escape or
/// bytes that are not UTF-8.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = raw.get(at + 1..at + 3)?;
            // `from_str_radix` alone would accept a sign (`%+f`).
            if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}
