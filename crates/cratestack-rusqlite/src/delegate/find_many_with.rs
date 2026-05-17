//! `FindManyWith` — `FindMany` plus a side-loaded to-one relation. The
//! merge happens client-side after running the parent query and an
//! IN-list child query.

use cratestack_sql::{Filter, FilterExpr, IntoSqlValue, OrderClause};

use crate::{FromRusqliteRow, RusqliteError, RusqliteRuntime};

use super::find_many::FindMany;

pub struct FindManyWith<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> {
    pub(super) parent: FindMany<'a, M, PK>,
    pub(super) relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
}

impl<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static>
    FindManyWith<'a, M, PK, Rel, RelPK>
{
    pub fn where_(mut self, filter: Filter) -> Self {
        self.parent = self.parent.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.parent = self.parent.where_expr(filter);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.parent = self.parent.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.parent = self.parent.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.parent = self.parent.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.parent = self.parent.offset(offset);
        self
    }

    /// `FOR UPDATE` is a no-op on embedded SQLite — preserved for
    /// cross-backend ergonomics. See [`FindMany::for_update`].
    pub fn for_update(self) -> Self {
        self
    }

    pub fn run(self) -> Result<Vec<(M, Option<Rel>)>, RusqliteError>
    where
        M: FromRusqliteRow + Clone,
        Rel: FromRusqliteRow + Clone + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Clone + std::cmp::Eq + std::hash::Hash + IntoSqlValue,
    {
        let runtime = self.parent.runtime;
        let relation = self.relation;
        let parents = self.parent.run()?;
        run_side_load(runtime, parents, relation, None::<&rusqlite::Connection>)
    }

    pub fn run_in_tx(
        self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<(M, Option<Rel>)>, RusqliteError>
    where
        M: FromRusqliteRow + Clone,
        Rel: FromRusqliteRow + Clone + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Clone + std::cmp::Eq + std::hash::Hash + IntoSqlValue,
    {
        let runtime = self.parent.runtime;
        let relation = self.relation;
        let parents = self.parent.run_in_tx(conn)?;
        run_side_load(runtime, parents, relation, Some(conn))
    }
}

fn run_side_load<M, Rel, RelPK>(
    runtime: &RusqliteRuntime,
    parents: Vec<M>,
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    conn: Option<&rusqlite::Connection>,
) -> Result<Vec<(M, Option<Rel>)>, RusqliteError>
where
    M: FromRusqliteRow + Clone,
    Rel: FromRusqliteRow + Clone + cratestack_sql::ModelPrimaryKey<RelPK>,
    RelPK: Clone + std::cmp::Eq + std::hash::Hash + IntoSqlValue,
{
    // Same shape as the sqlx implementation: collect distinct FK
    // values, side-load related rows via the runtime's IN-list path,
    // then merge by extracted primary key in memory.
    let mut fk_values: Vec<RelPK> = Vec::new();
    let mut seen: std::collections::HashSet<RelPK> = std::collections::HashSet::new();
    for parent in &parents {
        if let Some(fk) = (relation.parent_fk_extract)(parent)
            && seen.insert(fk.clone())
        {
            fk_values.push(fk);
        }
    }

    let by_pk: std::collections::HashMap<RelPK, Rel> = if fk_values.is_empty() {
        std::collections::HashMap::new()
    } else {
        let primary_key = relation.related_descriptor.primary_key;
        let mut probe = FindMany {
            runtime,
            descriptor: relation.related_descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };
        probe.filters.push(FilterExpr::from(cratestack_sql::Filter {
            column: primary_key,
            op: cratestack_sql::FilterOp::In,
            value: cratestack_sql::FilterValue::Many(
                fk_values
                    .iter()
                    .cloned()
                    .map(cratestack_sql::IntoSqlValue::into_sql_value)
                    .collect(),
            ),
        }));
        let related_rows = match conn {
            Some(conn) => probe.run_in_tx(conn)?,
            None => probe.run()?,
        };
        related_rows
            .into_iter()
            .map(|r| {
                let pk = cratestack_sql::ModelPrimaryKey::primary_key(&r);
                (pk, r)
            })
            .collect()
    };

    Ok(parents
        .into_iter()
        .map(|m| {
            let related = (relation.parent_fk_extract)(&m).and_then(|fk| by_pk.get(&fk).cloned());
            (m, related)
        })
        .collect())
}
