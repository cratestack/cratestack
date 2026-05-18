//! Embedded `ViewDelegate` — the per-view entry point for SQLite
//! (ADR-0003). Mirrors the sqlx delegate's read-only surface
//! (`find_many`, `find_unique`) but never exposes `refresh()` since
//! SQLite has no materialized views — the macro's embedded composer
//! hard-errors at expansion time on `@@materialized` views, so this
//! delegate never sees one in practice.

use cratestack_sql::ViewDescriptor;

use crate::RusqliteRuntime;

use super::find_many::FindMany;
use super::find_unique::FindUnique;

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

    /// See [`super::view::ViewDelegate`] in the sqlx side — emitted
    /// only for views with a primary key (i.e. not `@@no_unique`).
    pub fn find_unique(&self, id: PK) -> FindUnique<'a, V, PK> {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }
}
