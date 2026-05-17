//! Two-query side-load merge used by `FindMany::include` /
//! `FindManyWith::run`. The related rows go through a normal
//! `find_many` so the related model's read policy + soft-delete apply
//! for free.

use crate::{FilterExpr, SqlxRuntime, sqlx};

use super::find_many::FindMany;

pub(super) async fn run_side_load<'tx, M, Rel, RelPK>(
    runtime: &SqlxRuntime,
    parents: &[M],
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ctx: &cratestack_core::CoolContext,
    tx: Option<&mut sqlx::Transaction<'tx, sqlx::Postgres>>,
) -> Result<Vec<(M, Option<Rel>)>, cratestack_core::CoolError>
where
    M: 'static + Clone,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    Rel: 'static + Clone,
    for<'r> Rel: Send
        + Unpin
        + sqlx::FromRow<'r, sqlx::postgres::PgRow>
        + cratestack_sql::ModelPrimaryKey<RelPK>,
    RelPK: 'static
        + Send
        + Clone
        + std::cmp::Eq
        + std::hash::Hash
        + cratestack_sql::IntoSqlValue
        + sqlx::Type<sqlx::Postgres>
        + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    // Collect distinct non-null FK values from the parent rows. Dedup
    // preserves the per-parent merge step but trims the IN-list — for
    // batchy dispatchers where many parents share a target subscription,
    // this can collapse the side-load to a single matched row.
    let mut fk_values: Vec<RelPK> = Vec::new();
    let mut seen: std::collections::HashSet<RelPK> = std::collections::HashSet::new();
    for parent in parents {
        if let Some(fk) = (relation.parent_fk_extract)(parent)
            && seen.insert(fk.clone())
        {
            fk_values.push(fk);
        }
    }

    let by_pk: std::collections::HashMap<RelPK, Rel> = if fk_values.is_empty() {
        std::collections::HashMap::new()
    } else {
        // The side-load runs under the same read policy as a normal
        // find_many on the related descriptor, so rows the caller
        // can't see drop out and surface as `None` on the merged side.
        let primary_key = relation.related_descriptor.primary_key;
        let related_rows: Vec<Rel> = {
            let mut probe = FindMany {
                runtime,
                descriptor: relation.related_descriptor,
                filters: Vec::new(),
                order_by: Vec::new(),
                limit: None,
                offset: None,
                for_update: false,
            };
            probe.filters.push(FilterExpr::from(crate::Filter {
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
            match tx {
                Some(tx) => probe.run_in_tx(tx, ctx).await?,
                None => probe.run(ctx).await?,
            }
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
        .iter()
        .map(|m| {
            let related = (relation.parent_fk_extract)(m)
                .and_then(|fk| by_pk.get(&fk))
                .cloned();
            (m.clone(), related)
        })
        .collect())
}
