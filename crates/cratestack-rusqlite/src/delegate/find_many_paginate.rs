//! `FindMany::paginate` — real `COUNT(*)` + paginated `SELECT`, assembled
//! into a `Page<M>` with accurate `PageInfo`. Split out from
//! `find_many.rs` per the repo's 200-LoC file convention.

use cratestack_core::{MAX_LIST_LIMIT, Page, PageInfo, PageInput};
use cratestack_sql::SqliteDialect;
use rusqlite::params_from_iter;

use crate::render::{render_count, render_select};
use crate::{FromRusqliteRow, RusqliteError, SqlValueParam};

use super::find_many::FindMany;

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
    /// Real `COUNT(*)` over the same filters, then the paginated
    /// `SELECT`, assembled into a `Page<M>` with accurate `PageInfo` —
    /// the embedded backend's counterpart to what a `@@paged` model's
    /// generated server `list` route computes. `limit`/`offset` come
    /// from `page` via `PageInput::resolve`, clamped to
    /// [`MAX_LIST_LIMIT`] the same way generated `list` routes already
    /// clamp theirs.
    ///
    /// Available unconditionally on every model's (and view's) delegate
    /// — unlike REST/RPC/gRPC there's no advance wire contract to fix
    /// per model here: the caller is the same binary that defines the
    /// schema, choosing per call site whether it wants `Page<M>` (this
    /// method) or a bare `Vec<M>` (`.run()`).
    ///
    /// Both statements run inside one `with_connection` borrow so the
    /// count and the page it describes are never split by a concurrent
    /// write racing in between the two queries.
    pub fn paginate(self, page: PageInput) -> Result<Page<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let (limit, offset) = page.resolve(MAX_LIST_LIMIT);
        let dialect = SqliteDialect;
        let (count_sql, count_binds) = render_count(&dialect, self.descriptor, &self.filters);
        let (select_sql, select_binds) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            Some(limit),
            Some(offset),
        );
        let (total_count, items) = self.runtime.with_connection(|conn| {
            let total_count: i64 = {
                let mut stmt = conn.prepare(&count_sql)?;
                let bind_iter = count_binds.iter().map(SqlValueParam);
                stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?
            };
            let mut stmt = conn.prepare(&select_sql)?;
            let bind_iter = select_binds.iter().map(SqlValueParam);
            let items = stmt
                .query_map(params_from_iter(bind_iter), |row| M::from_rusqlite_row(row))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((total_count, items))
        })?;

        Ok(assemble_page(items, limit, offset, total_count))
    }

    /// Transaction-scoped variant of [`Self::paginate`] — mirrors
    /// `.run_in_tx`'s shape for cross-backend ergonomics.
    pub fn paginate_in_tx(
        self,
        page: PageInput,
        conn: &rusqlite::Connection,
    ) -> Result<Page<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let (limit, offset) = page.resolve(MAX_LIST_LIMIT);
        let dialect = SqliteDialect;
        let (count_sql, count_binds) = render_count(&dialect, self.descriptor, &self.filters);
        let (select_sql, select_binds) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            Some(limit),
            Some(offset),
        );

        let total_count: i64 = {
            let mut stmt = conn.prepare(&count_sql)?;
            let bind_iter = count_binds.iter().map(SqlValueParam);
            stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?
        };
        let mut stmt = conn.prepare(&select_sql)?;
        let bind_iter = select_binds.iter().map(SqlValueParam);
        let items = stmt
            .query_map(params_from_iter(bind_iter), |row| M::from_rusqlite_row(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(assemble_page(items, limit, offset, total_count))
    }
}

fn assemble_page<M>(items: Vec<M>, limit: i64, offset: i64, total_count: i64) -> Page<M> {
    Page::new(
        items,
        PageInfo {
            limit: Some(limit),
            offset: Some(offset),
            has_next_page: offset + limit < total_count,
            has_previous_page: offset > 0,
        },
    )
    .with_total_count(Some(total_count))
}
