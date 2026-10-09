//! Update and delete: the preflight denies, and the policy spliced into the
//! statement leaves the matching row alone.

use cratestack_core::{BigInt, CratestackError, Value};

use super::fixture::{ALL, claim, descriptor, leak, reseed};
use crate::query::{authorize_record_action, push_action_policy_query};
use crate::sqlx::{self, PgPool};
use crate::{PolicyLiteral, ReadPredicate, SqlxRuntime};

pub(super) async fn negated_policies_deny_on_update_and_delete(pool: &PgPool) {
    let runtime = SqlxRuntime::new(pool.clone());
    for value in ALL {
        let ne_literal = leak(ReadPredicate::FieldNeLiteral {
            column: "amount",
            value: PolicyLiteral::Int(value),
        });
        let ne_auth = leak(ReadPredicate::FieldNeAuth {
            column: "amount",
            auth_field: "tenant",
        });
        let other = ALL.iter().copied().find(|v| *v != value).unwrap();
        let ctx = claim(Value::Int(value));
        reseed(pool).await;

        for (allow, label) in [(ne_literal, "!= literal"), (ne_auth, "!= auth()")] {
            for action in ["update", "delete"] {
                let target = descriptor(allow, &[]);
                let denied = authorize_record_action(
                    &runtime,
                    target,
                    BigInt::new(value),
                    allow,
                    &[],
                    &ctx,
                    action,
                )
                .await;
                assert!(
                    matches!(denied, Err(CratestackError::Forbidden(_))),
                    "{action} `amount {label}` {value} must deny the row where amount = {value}: {denied:?}"
                );
                authorize_record_action(
                    &runtime,
                    target,
                    BigInt::new(other),
                    allow,
                    &[],
                    &ctx,
                    action,
                )
                .await
                .unwrap_or_else(|e| panic!("{action} `{label}` admits {other}: {e:?}"));
            }
        }

        let mut update = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "UPDATE bigint_policy_rows SET touched = TRUE WHERE ",
        );
        push_action_policy_query(&mut update, ne_literal, &[], &ctx);
        let touched = update.build().execute(pool).await.unwrap().rows_affected();
        assert_eq!(touched, 5, "UPDATE skips the row where amount = {value}");
        let untouched: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM bigint_policy_rows WHERE NOT touched")
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(untouched, vec![value]);

        let mut delete =
            sqlx::QueryBuilder::<sqlx::Postgres>::new("DELETE FROM bigint_policy_rows WHERE ");
        push_action_policy_query(&mut delete, ne_auth, &[], &ctx);
        let removed = delete.build().execute(pool).await.unwrap().rows_affected();
        assert_eq!(removed, 5, "DELETE skips the row where amount = {value}");
        let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM bigint_policy_rows")
            .fetch_all(pool)
            .await
            .unwrap();
        assert_eq!(left, vec![value]);
    }
}
