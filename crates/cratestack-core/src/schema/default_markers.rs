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

/// The marker function called by the *inner text* of a `@default(...)`
/// attribute — the text between the outer parentheses — or `None` if it is
/// not one.
///
/// A call is: optional leading whitespace, a name of `[A-Za-z0-9_]`,
/// optional whitespace, then `(`. Case-sensitive, matching the repo's
/// exact-text philosophy for the closed attribute surface (GHSA-69g4-xvcm-vm2j).
pub fn default_marker(inner: &str) -> Option<DefaultMarker> {
    let trimmed = inner.trim_start();
    let name_len = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    let (name, rest) = trimmed.split_at(name_len);
    if !rest.trim_start().starts_with('(') {
        return None;
    }
    match name {
        "dbgenerated" => Some(DefaultMarker::DbGenerated),
        "autoincrement" => Some(DefaultMarker::Autoincrement),
        _ => None,
    }
}

#[cfg(test)]
#[path = "default_markers_tests.rs"]
mod tests;
