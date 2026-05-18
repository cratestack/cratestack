//! View IR — the `view <Name> from <Model>, ... { ... }` block.
//!
//! Views are read-only, SQL-defined projections over one or more
//! existing `model` blocks (see ADR-0003). Their fields, attributes,
//! and span tracking mirror [`Model`](super::Model); the extra state is
//! the explicit source-model dependency list and the per-backend SQL
//! bodies parsed out of `@@server_sql` / `@@embedded_sql` / `@@sql`.

use serde::{Deserialize, Serialize};

use super::{Attribute, Field, SourceSpan};

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
    /// `None` means the view is embedded-only.
    pub fn server_sql(&self) -> Option<&str> {
        self.body_attribute("@@server_sql")
            .or_else(|| self.body_attribute("@@sql"))
    }

    /// Returns the SQL body declared via `@@embedded_sql("…")`, or the
    /// `@@sql("…")` shorthand if no backend-specific body is set.
    /// `None` means the view is server-only.
    pub fn embedded_sql(&self) -> Option<&str> {
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

    fn body_attribute(&self, prefix: &str) -> Option<&str> {
        self.attributes
            .iter()
            .filter(|attr| attr.raw.starts_with(prefix))
            .find_map(|attr| extract_sql_body(&attr.raw, prefix))
    }

    fn has_bare_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|attr| {
            let trimmed = attr.raw.trim();
            trimmed == name || trimmed.starts_with(&format!("{name}(")) || trimmed == name
        })
    }
}

/// Extract the SQL body from an attribute like `@@server_sql("…")`.
/// Accepts both `"single-line"` and `"""multi-line"""` strings. The
/// outer quotes are stripped; embedded newlines and quotes are
/// preserved verbatim.
fn extract_sql_body<'a>(raw: &'a str, prefix: &str) -> Option<&'a str> {
    let after_prefix = raw.strip_prefix(prefix)?.trim_start();
    let inside_parens = after_prefix
        .strip_prefix('(')?
        .rsplit_once(')')
        .map(|(body, _tail)| body)?
        .trim();
    if let Some(rest) = inside_parens.strip_prefix("\"\"\"") {
        rest.strip_suffix("\"\"\"")
    } else if let Some(rest) = inside_parens.strip_prefix('"') {
        rest.strip_suffix('"')
    } else {
        None
    }
}
