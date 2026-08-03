//! Built-in procedure-argument type for search-with-filters: the
//! `FindMany<Model>` schema syntax (`.cstack`), the procedure-argument-only
//! counterpart to `Page<T>`/`PageInput`. Composes with `PageInput` rather
//! than absorbing it — a procedure wanting both filtering and pagination
//! declares two arguments, e.g. `procedure search(query: FindMany<Post>,
//! page: PageInput): Page<Post>`.
//!
//! Reuses the exact same `where=`/`sort=` string grammar the generated
//! `list` route's query string already accepts
//! (`cratestack-axum::query::parse_filter_expression`), rather than a new
//! structured filter representation, so a caller who already knows how to
//! filter a REST `list` route already knows how to fill this in.

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// `M` carries no data — it exists so `cratestack-macros`-generated code
/// can select the right model's filter/sort validation at compile time
/// (mirrors `Page<M>`'s own phantom-free-of-data generic parameter).
///
/// `Debug`/`Clone`/`PartialEq`/`Eq`/`Default` are hand-written below
/// rather than derived: `#[derive(...)]` adds a `M: Trait` bound for
/// *every* generic parameter regardless of how it's actually used, which
/// would wrongly force every model type used with `FindMany<Model>` in a
/// `.cstack` schema to itself implement `Debug`/`Clone`/`Default`/etc. —
/// `M` here is a compile-time-only marker (`PhantomData<fn() -> M>`),
/// never constructed or stored.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct FindManyInput<M> {
    /// Same grammar as a REST `list` route's `?where=` query parameter:
    /// `field=value` predicates joined with `,` (AND) / `|` (OR), and
    /// `not(...)` negation.
    pub r#where: Option<String>,
    /// Same grammar as a REST `list` route's `?sort=` query parameter:
    /// comma-separated field names, `-` prefix for descending.
    pub order_by: Option<String>,
    #[serde(skip)]
    pub(crate) _marker: PhantomData<fn() -> M>,
}

impl<M> std::fmt::Debug for FindManyInput<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FindManyInput")
            .field("where", &self.r#where)
            .field("order_by", &self.order_by)
            .finish()
    }
}

impl<M> Clone for FindManyInput<M> {
    fn clone(&self) -> Self {
        Self {
            r#where: self.r#where.clone(),
            order_by: self.order_by.clone(),
            _marker: PhantomData,
        }
    }
}

impl<M> PartialEq for FindManyInput<M> {
    fn eq(&self, other: &Self) -> bool {
        self.r#where == other.r#where && self.order_by == other.order_by
    }
}

impl<M> Eq for FindManyInput<M> {}

impl<M> Default for FindManyInput<M> {
    fn default() -> Self {
        Self {
            r#where: None,
            order_by: None,
            _marker: PhantomData,
        }
    }
}

impl<M> FindManyInput<M> {
    /// `_marker` is private (there is nothing meaningful a caller could
    /// set it to), so this — not struct-literal syntax — is the public
    /// constructor.
    pub fn new(r#where: Option<String>, order_by: Option<String>) -> Self {
        Self {
            r#where,
            order_by,
            _marker: PhantomData,
        }
    }
}
