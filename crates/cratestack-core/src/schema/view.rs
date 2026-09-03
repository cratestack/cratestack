//! View IR — the `view <Name> from <Model>, ... { ... }` block.
//!
//! Views are read-only, SQL-defined projections over one or more
//! existing `model` blocks (see ADR-0003). Their fields, attributes,
//! and span tracking mirror [`Model`](super::Model); the extra state is
//! the explicit source-model dependency list and the per-backend SQL
//! bodies parsed out of `@@server_sql` / `@@embedded_sql` / `@@sql`.

use serde::{Deserialize, Serialize};

use super::sql_body::extract_sql_body;
use super::{Attribute, Field, SourceSpan};

/// Every attribute name that carries a view's SQL body. Mirrors
/// `cratestack-parser`'s `SQL_ATTRS`, which drives the multi-line
/// capture; kept here too because [`View::has_sql_attribute`] has to
/// recognise a malformed attribute the parser already accepted.
const SQL_ATTRIBUTE_PREFIXES: &[&str] = &["@@server_sql", "@@embedded_sql", "@@sql"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub docs: Vec<String>,
    pub name: String,
    pub name_span: SourceSpan,
    /// The `from <Model>, <Model>, ...` dependency list. Source model
    /// names are stored as raw identifiers — the validator resolves
    /// them against the schema's models. Carries spans so error
    /// reporting can point at the offending identifier.
    pub sources: Vec<ViewSource>,
    pub fields: Vec<Field>,
    /// Block-level attributes — `@@server_sql`, `@@embedded_sql`,
    /// `@@sql`, `@@materialized`, `@@no_unique`, `@@allow("read", …)`.
    /// Stored raw; helper methods below extract typed views.
    pub attributes: Vec<Attribute>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSource {
    pub name: String,
    pub name_span: SourceSpan,
}

impl View {
    /// Returns the SQL body declared via `@@server_sql("…")`, or the
    /// `@@sql("…")` shorthand if no backend-specific body is set.
    /// `None` means the view is embedded-only **or** that a body is
    /// present but malformed — the two are distinguished by
    /// [`has_sql_attribute`](Self::has_sql_attribute), which the parser's
    /// semantic pass uses to reject the second case rather than silently
    /// treating it as the first (cratestack#867 review finding 2).
    ///
    /// Owned rather than borrowed because the single-line form unescapes
    /// `\"`/`\\` — see [`extract_sql_body`].
    pub fn server_sql(&self) -> Option<String> {
        self.body_attribute("@@server_sql")
            .or_else(|| self.body_attribute("@@sql"))
    }

    /// Returns the SQL body declared via `@@embedded_sql("…")`, or the
    /// `@@sql("…")` shorthand if no backend-specific body is set.
    /// `None` means the view is server-only, or malformed — see
    /// [`server_sql`](Self::server_sql).
    pub fn embedded_sql(&self) -> Option<String> {
        self.body_attribute("@@embedded_sql")
            .or_else(|| self.body_attribute("@@sql"))
    }

    /// `true` if the view was declared with `@@materialized`.
    /// Materialized views are server-only — the embedded composer
    /// emits a hard compile error when it encounters one.
    pub fn is_materialized(&self) -> bool {
        self.has_bare_attribute("@@materialized")
    }

    /// `true` if the view opts out of a natural unique key via
    /// `@@no_unique`. Drops `find_unique` from the generated delegate.
    pub fn no_unique(&self) -> bool {
        self.has_bare_attribute("@@no_unique")
    }

    /// Whether *any* `@@…sql` attribute is written on this view,
    /// regardless of whether its argument parses as a quoted string.
    ///
    /// The gap this closes: `server_sql()`/`embedded_sql()` return `None`
    /// both for "no body declared" and for "body declared but malformed"
    /// (`@@server_sql(SELECT 1)`, unquoted). Without a way to tell those
    /// apart, a typo'd body reads as an embedded-only view and is skipped
    /// by the server composer with no diagnostic at all.
    pub fn has_sql_attribute(&self) -> bool {
        self.attributes.iter().any(|attr| {
            let trimmed = attr.raw.trim_start();
            SQL_ATTRIBUTE_PREFIXES
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
        })
    }

    /// Trims leading whitespace exactly as [`has_sql_attribute`] does.
    /// The two disagreeing is how an indented `@@server_sql` would be
    /// reported as "declared but malformed" while yielding no body at all
    /// — `starts_with` would miss it here, `trim_start` would find it
    /// there (cratestack#870 review nit 5).
    ///
    /// [`has_sql_attribute`]: Self::has_sql_attribute
    fn body_attribute(&self, prefix: &str) -> Option<String> {
        self.attributes
            .iter()
            .map(|attr| attr.raw.trim_start())
            .filter(|raw| raw.starts_with(prefix))
            .find_map(|raw| extract_sql_body(raw, prefix))
    }

    fn has_bare_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|attr| {
            let trimmed = attr.raw.trim();
            trimmed == name || trimmed.starts_with(&format!("{name}("))
        })
    }
}
