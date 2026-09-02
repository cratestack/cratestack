//! Every `query`-block-aware branch of the language server, in one place
//! (cratestack#867).
//!
//! Each function here is the `query` half of a sibling module's per-construct
//! walk — `hover`'s symbol lookup, `definition`'s name resolution,
//! `references`' type-reference index, `symbol_target`'s cursor resolution,
//! `semantic_tokens`' entry collection. They live together rather than
//! inline for two reasons, in this order:
//!
//! 1. **The rules are one rule.** A query's name is a FUNCTION for
//!    colouring, a declaration for navigation, and a rename target — three
//!    files' worth of code expressing one fact. Splitting it across those
//!    files means a future change (say, colouring the `@@sql` body) has to
//!    be remembered in five places.
//! 2. The sibling modules were each within a few lines of the workspace's
//!    200-line ceiling, and adding a per-construct loop to five of them
//!    would have pushed most over.
//!
//! **What is deliberately absent: the SQL body.** It is opaque text to this
//! server — no tokens, no navigation, no completion inside it. Doing
//! otherwise means embedding a SQL parser for a dialect the framework
//! itself never parses, which is exactly the cost
//! `docs/design/declarative-custom-query.md` §3 prices out and rejects.

use cratestack_core::{Schema, SourceSpan};

use crate::hover::named_type_symbol;
use crate::state::SymbolInfo;
use crate::text::span_contains;
use crate::type_ref::{
    collect_type_ref_spans, nested_type_reference_name_at_offset, render_type_ref,
    type_ref_at_offset,
};

/// Hover target under `offset`: a parameter, a named type inside the
/// signature, or the query's own name. Same three targets a procedure has.
pub(crate) fn hover_symbol(schema: &Schema, offset: usize) -> Option<SymbolInfo> {
    for query in &schema.queries {
        for arg in &query.args {
            if span_contains(arg.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &arg.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(arg.span, offset) {
                return Some(SymbolInfo {
                    kind: "argument",
                    name: arg.name.clone(),
                    detail: render_type_ref(&arg.ty),
                    docs: arg.docs.clone(),
                    selection_span: arg.name_span,
                });
            }
        }
        if type_ref_at_offset(&query.result_type, offset)
            && let Some(symbol) = named_type_symbol(schema, &query.result_type, offset)
        {
            return Some(symbol);
        }
        if span_contains(query.name_span, offset) {
            return Some(SymbolInfo {
                kind: "query",
                name: query.name.clone(),
                detail: format!("query -> {}", render_type_ref(&query.result_type)),
                docs: query.docs.clone(),
                selection_span: query.name_span,
            });
        }
    }
    None
}

/// The span declaring `word`, when `word` names a query or one of its
/// parameters. Parameters matter here specifically because that is what an
/// `@allow(auth().subjectId == userId)` predicate refers to.
pub(crate) fn declaration_span(schema: &Schema, word: &str) -> Option<SourceSpan> {
    for query in &schema.queries {
        if query.name == word {
            return Some(query.name_span);
        }
        if let Some(arg) = query.args.iter().find(|arg| arg.name == word) {
            return Some(arg.name_span);
        }
    }
    None
}

/// The declaration name a type reference under `offset` points at, when
/// that reference is inside a query's signature.
///
/// A query's result type is a reference to a `type` block, so this is what
/// makes go-to-definition land there — and, via
/// [`collect_type_reference_spans`], what stops a rename of that `type`
/// skipping the query and leaving the schema uncompilable.
pub(crate) fn type_reference_at(schema: &Schema, offset: usize) -> Option<&str> {
    for query in &schema.queries {
        if let Some(name) = nested_type_reference_name_at_offset(&query.result_type, offset) {
            return Some(name);
        }
        for arg in &query.args {
            if let Some(name) = nested_type_reference_name_at_offset(&arg.ty, offset) {
                return Some(name);
            }
        }
    }
    None
}

/// Every span inside a query signature that references the type `name`.
///
/// A query's signature references types exactly as a procedure's does, and
/// rename is built on this index — so omitting queries here would mean
/// renaming a `type` silently skipped the query that returns it, leaving
/// the schema uncompilable and the author looking at a diff that seemed
/// complete.
pub(crate) fn collect_type_reference_spans(
    schema: &Schema,
    name: &str,
    spans: &mut Vec<SourceSpan>,
) {
    for query in &schema.queries {
        collect_type_ref_spans(&query.result_type, name, spans);
        for arg in &query.args {
            collect_type_ref_spans(&arg.ty, name, spans);
        }
    }
}

/// The query whose own name span contains `offset`.
pub(crate) fn declaration_at(schema: &Schema, offset: usize) -> Option<&str> {
    schema
        .queries
        .iter()
        .find(|query| span_contains(query.name_span, offset))
        .map(|query| query.name.as_str())
}

/// Whether the schema declares a query called `word` — the last-resort
/// "is this bare word a symbol at all" check.
pub(crate) fn declares(schema: &Schema, word: &str) -> bool {
    schema.queries.iter().any(|query| query.name == word)
}

/// Semantic-token entries for every query: name as FUNCTION, parameters as
/// PARAMETER, result and parameter types resolved.
pub(crate) fn collect_semantic_tokens(
    schema: &Schema,
    function: u32,
    parameter: u32,
    entries: &mut Vec<(SourceSpan, u32)>,
) {
    for query in &schema.queries {
        entries.push((query.name_span, function));
        crate::semantic_tokens::collect_type_ref(schema, &query.result_type, entries);
        for arg in &query.args {
            entries.push((arg.name_span, parameter));
            crate::semantic_tokens::collect_type_ref(schema, &arg.ty, entries);
        }
    }
}
