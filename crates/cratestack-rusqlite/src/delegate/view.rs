//! Embedded view delegates — the per-view entry points for SQLite
//! (ADR-0003). Mirror the sqlx delegate's read-only surface but
//! never expose `refresh()` (SQLite has no materialized views; the
//! macro's embedded composer hard-errors at expansion time on
//! `@@materialized`).
//!
//! Two structs — same split as the sqlx side — so `@@no_unique`
//! views literally cannot expose `find_unique` at the type level.

use cratestack_sql::ViewDescriptor;

use crate::RusqliteRuntime;

use super::find_many::FindMany;
use super::find_unique::FindUnique;

/// Embedded delegate for views with an `@id` field.
#[derive(Clone, Copy)]
pub struct ViewDelegate<'a, V: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ViewDescriptor<V, PK>,
}

impl<'a, V: 'static, PK: 'static> ViewDelegate<'a, V, PK> {
    pub fn new(runtime: &'a RusqliteRuntime, descriptor: &'static ViewDescriptor<V, PK>) -> Self {
        Self {
            runtime,
            descriptor,
        }
    }

    pub fn descriptor(&self) -> &'static ViewDescriptor<V, PK> {
        self.descriptor
    }

    pub fn find_many(&self) -> FindMany<'a, V, PK> {
        FindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    pub fn find_unique(&self, id: PK) -> FindUnique<'a, V, PK> {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }
}

/// Embedded delegate for views declared `@@no_unique`. Only
/// `find_many` — `find_unique` is absent at the type level (same
/// rationale as the sqlx-side [`super::view::ViewDelegateNoUnique`]).
#[derive(Clone, Copy)]
pub struct ViewDelegateNoUnique<'a, V: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ViewDescriptor<V, ()>,
}

impl<'a, V: 'static> ViewDelegateNoUnique<'a, V> {
    pub fn new(runtime: &'a RusqliteRuntime, descriptor: &'static ViewDescriptor<V, ()>) -> Self {
        Self {
            runtime,
            descriptor,
        }
    }

    pub fn descriptor(&self) -> &'static ViewDescriptor<V, ()> {
        self.descriptor
    }

    pub fn find_many(&self) -> FindMany<'a, V, ()> {
        FindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }
}
