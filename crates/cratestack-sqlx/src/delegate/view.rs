//! `ViewDelegate` — the per-view entry point handed out by the
//! generated `Cratestack::views().<view>()` accessor (ADR-0003).
//!
//! Views are read-only. The delegate exposes `find_many` and
//! `find_unique` only — never any write primitive. The read-only-ness
//! guarantee is at the *type* level: the builder methods return the
//! same `FindMany` / `FindUnique` types used by `ModelDelegate`, but
//! the descriptor carried through is a `ViewDescriptor<V, PK>` which
//! does not implement `WriteSource`. The macro can't accidentally
//! wire a view through a write builder because the bound doesn't
//! hold.
//!
//! For `@@materialized` views (server-only, ADR-0003 §"Materialized
//! views") the delegate also exposes `refresh()` which emits
//! `REFRESH MATERIALIZED VIEW CONCURRENTLY <name>`. Concurrent
//! refresh requires the unique index the materialized DDL emits on
//! the `@id` column, which is why `@@materialized` + `@@no_unique`
//! is a parse-time error.

use cratestack_core::CoolError;
use cratestack_sql::ViewDescriptor;

use crate::{FindMany, FindUnique, SqlxRuntime, sqlx};

/// View delegate for views that declared an `@id` field. Exposes
/// `find_many` + `find_unique` (and `refresh()` on materialized
/// views). Views declared `@@no_unique` get [`ViewDelegateNoUnique`]
/// instead, which omits `find_unique` at the type level so a call
/// like `runtime.views().<v>().find_unique(())` is a compile error
/// rather than a runtime "WHERE  = $1" footgun.
#[derive(Clone, Copy)]
pub struct ViewDelegate<'a, V: 'static, PK: 'static> {
    pub(super) runtime: &'a SqlxRuntime,
    pub(super) descriptor: &'static ViewDescriptor<V, PK>,
}

impl<'a, V: 'static, PK: 'static> ViewDelegate<'a, V, PK> {
    pub fn new(runtime: &'a SqlxRuntime, descriptor: &'static ViewDescriptor<V, PK>) -> Self {
        Self {
            runtime,
            descriptor,
        }
    }

    /// The typed descriptor the delegate was constructed with.
    /// Useful for callers that need to inspect view metadata (e.g.
    /// `is_materialized`, `source_tables`) without going through the
    /// runtime.
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
            for_update: false,
        }
    }

    /// Single-row lookup by primary key. Only available on views
    /// with an `@id` field — `@@no_unique` views get
    /// [`ViewDelegateNoUnique`], which doesn't expose this method.
    pub fn find_unique(&self, id: PK) -> FindUnique<'a, V, PK> {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
            for_update: false,
            policy_kind: crate::query::ReadPolicyKind::Detail,
        }
    }

    /// `REFRESH MATERIALIZED VIEW CONCURRENTLY <name>` — only valid
    /// on `@@materialized` views. Concurrent refresh holds an
    /// `ACCESS SHARE` lock (instead of the `ACCESS EXCLUSIVE`
    /// non-concurrent refresh takes), letting readers continue
    /// against the previous snapshot while the rebuild runs.
    ///
    /// Returns a `Forbidden` error if called on a non-materialized
    /// view — the macro emits this method unconditionally for
    /// `ViewDescriptor` consumers, with the gate enforced at runtime
    /// so the wire contract is uniform. (At codegen time the macro
    /// can also choose to omit the method entirely on non-materialized
    /// descriptors; the runtime gate is the safety net.)
    pub async fn refresh(&self) -> Result<(), CoolError> {
        if !self.descriptor.is_materialized {
            return Err(CoolError::Forbidden(format!(
                "view `{}` is not `@@materialized`; refresh() is only valid on materialized views",
                self.descriptor.view_name
            )));
        }
        let sql = format!(
            "REFRESH MATERIALIZED VIEW CONCURRENTLY {}",
            self.descriptor.view_name
        );
        sqlx::query(&sql)
            .execute(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        Ok(())
    }
}

/// View delegate for views declared `@@no_unique`. Exposes only
/// `find_many` — `find_unique` and `refresh()` are absent at the type
/// level because:
///
/// - `find_unique` needs an `@id` field, which `@@no_unique` views
///   opt out of (validator-enforced).
/// - `@@materialized` + `@@no_unique` is a parse-time error
///   (concurrent refresh requires a unique index), so a
///   `ViewDelegateNoUnique` can never be materialized.
///
/// `PK` is fixed to `()` because the underlying `ViewDescriptor<V, ()>`
/// stores an empty `primary_key` string — preventing any code path
/// from constructing one with a real PK type.
#[derive(Clone, Copy)]
pub struct ViewDelegateNoUnique<'a, V: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ViewDescriptor<V, ()>,
}

impl<'a, V: 'static> ViewDelegateNoUnique<'a, V> {
    pub fn new(runtime: &'a SqlxRuntime, descriptor: &'static ViewDescriptor<V, ()>) -> Self {
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
            for_update: false,
        }
    }
}
