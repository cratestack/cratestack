//! The `@default(...)` marker functions — spellings that are *not* real
//! database functions and must never reach a `DEFAULT` clause verbatim.
//!
//! There are two:
//!
//! - `dbgenerated()` — the sanctioned marker meaning "the column's value is
//!   generated at the database level" (a sequence, `GENERATED ... AS
//!   IDENTITY`, a trigger, hand-authored SQL). The emitter emits no
//!   `DEFAULT` clause for it and warns that the real default must exist.
//! - `autoincrement()` — a Prisma-style spelling that `cratestack check`
//!   refuses outright (cratestack#1128), because neither Postgres nor
//!   SQLite has such a function and the emitted `DEFAULT autoincrement()`
//!   fails at apply time. The emitter must never write it either.
//!
//! These are matched **whitespace-insensitively**. `attribute_text::split_attributes`
//! trims only the outer edges of each attribute (it does `piece.trim()` per
//! piece), so `@default(autoincrement ())` keeps the space between the name
//! and its opening parenthesis. A plain exact-string match would let that
//! spelling slip past both the parser validator and the migration emitter
//! and re-emit broken DDL. Because the two live in different crates, sharing
//! one matcher here is what stops them disagreeing again — each previously
//! hand-rolled the same exact-string check and shared the same blind spot.
//!
//! Classification only: the returned marker decides *what the spelling
//! means*, never the emitted text. A quoted literal (`'pending'`), a bare
//! word, or any other function (`now()`, `cs_cuid()`) is `None` and is left
//! to the caller's own literal/function handling, which is deliberate and
//! unchanged.

/// A `@default(...)` marker function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultMarker {
    /// `@default(dbgenerated())` — the sanctioned marker.
    DbGenerated,
    /// `@default(autoincrement())` — refused by the parser (cratestack#1128).
    Autoincrement,
}

/// A marker call split into its parts. `args` is the text between the
/// call's parentheses; `trailing` is whatever follows the matching `)`.
/// `closed` is false when the opening `(` has no matching `)`.
///
/// A well-formed marker is the *whole* default expression: no argument,
/// closed, nothing after it. `dbgenerated() || 'x'` or `dbgenerated()::text`
/// still classify as the marker (so an emitter never writes them as SQL),
/// and the parser refuses them by checking [`MarkerCall::is_bare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerCall<'a> {
    pub marker: DefaultMarker,
    pub args: &'a str,
    pub trailing: &'a str,
    pub closed: bool,
    /// The name has parentheses after it. `@default(autoincrement)` is the
    /// marker's bare word: an emitter would write it raw, so it classifies.
    pub called: bool,
    /// The name is spelled exactly (`dbgenerated`, not `DbGenerated`).
    pub canonical: bool,
}

impl DefaultMarker {
    /// The marker's exact spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::DbGenerated => "dbgenerated",
            Self::Autoincrement => "autoincrement",
        }
    }
}

impl MarkerCall<'_> {
    /// `name()` exactly and nothing else (whitespace aside).
    pub fn is_bare(&self) -> bool {
        self.called
            && self.canonical
            && self.closed
            && self.args.trim().is_empty()
            && self.trailing.trim().is_empty()
    }
}

/// Parse the *inner text* of a `@default(...)` attribute — the text between
/// the outer parentheses — as a marker call, or `None` if it does not start
/// with one.
///
/// A call is: optional leading whitespace, a name of `[A-Za-z0-9_]`,
/// optional whitespace, then `(`; the closing `)` is the one that balances
/// it. The name, or the name alone with nothing after it, also classifies.
///
/// Names match **case-insensitively**. Postgres folds unquoted identifiers,
/// so `DEFAULT AutoIncrement()` fails exactly like `autoincrement()`; an
/// exact-case match would let `AutoIncrement()` through as an ordinary
/// function. Only the exact spelling is accepted by the parser
/// ([`MarkerCall::canonical`], [`MarkerCall::is_bare`]), keeping the closed
/// attribute surface exact-text (GHSA-69g4-xvcm-vm2j).
pub fn marker_call(inner: &str) -> Option<MarkerCall<'_>> {
    let trimmed = inner.trim_start();
    let name_len = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    let (name, rest) = trimmed.split_at(name_len);
    let marker = [DefaultMarker::DbGenerated, DefaultMarker::Autoincrement]
        .into_iter()
        .find(|marker| name.eq_ignore_ascii_case(marker.name()))?;
    let canonical = name == marker.name();
    let after = rest.trim_start();
    let Some(body) = after.strip_prefix('(') else {
        return after.is_empty().then_some(MarkerCall {
            marker,
            args: "",
            trailing: "",
            closed: false,
            called: false,
            canonical,
        });
    };
    let mut depth = 1usize;
    for (index, c) in body.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(MarkerCall {
                        marker,
                        args: &body[..index],
                        trailing: &body[index + 1..],
                        closed: true,
                        called: true,
                        canonical,
                    });
                }
            }
            _ => {}
        }
    }
    Some(MarkerCall {
        marker,
        args: body,
        trailing: "",
        closed: false,
        called: true,
        canonical,
    })
}

/// The marker a `@default(...)` inner text starts with, if any. See
/// [`marker_call`] for the parts.
pub fn default_marker(inner: &str) -> Option<DefaultMarker> {
    marker_call(inner).map(|call| call.marker)
}

/// The trimmed inner text of a field's first `@default(...)` attribute.
pub fn default_inner(field: &crate::Field) -> Option<&str> {
    field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@default("))?
        .raw
        .strip_prefix("@default(")?
        .strip_suffix(')')
        .map(str::trim)
}

#[cfg(test)]
#[path = "default_markers_tests.rs"]
mod tests;
