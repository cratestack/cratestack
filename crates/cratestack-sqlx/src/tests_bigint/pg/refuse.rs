//! A string claim against a `BIGINT` column. The predicate carries no column
//! type, so the claim cannot be coerced; it must be refused, deterministically,
//! for `==` and for `!=`, and never match.
//!
//! The trap this pins: sqlx caches a persistent prepared statement by SQL text
//! alone. `read::negated_policies_deny_on_read` has already prepared
//! `... WHERE (amount != $1) ...` with `$1` as `int8` (an integer claim). If a
//! string claim reused that text it would be sent as binary `int8`: a 7-byte
//! string fails with 08P01, and an 8-byte one is read as an integer, which made
//! `!=` admit every row (measured before `claim_type_suffix` put `::text` in the
//! SQL). With the suffix the string claim is its own statement and Postgres
//! types `$1` as `text`, so every attempt is `operator does not exist` (42883).

use cratestack_core::{BigInt, Value};

use super::fixture::{claim, descriptor, leak, reseed};
use super::read::visible;
use crate::query::authorize_record_action;
use crate::sqlx::{self, PgPool};
use crate::{ReadPredicate, SqlxRuntime};

fn sqlstate(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .map(|code| code.to_string())
}

pub(super) async fn a_string_claim_is_refused_not_matched(pool: &PgPool) {
    reseed(pool).await;
    let runtime = SqlxRuntime::new(pool.clone());
    for predicate in [
        ReadPredicate::FieldNeAuth {
            column: "amount",
            auth_field: "tenant",
        },
        ReadPredicate::FieldEqAuth {
            column: "amount",
            auth_field: "tenant",
        },
    ] {
        let allow = leak(predicate);
        // An integer claim first, so this text is prepared (and cached) as int8.
        let rows = visible(pool, descriptor(allow, &[]), &claim(Value::Int(7))).await;
        assert!(
            rows.is_ok(),
            "{predicate:?} with an integer claim: {rows:?}"
        );

        // The 8-byte and 7-byte strings are the ones that used to be
        // reinterpreted or to fail at the protocol level; repeat each, since
        // the failure alternated with the statement cache.
        for text in ["12345678", "12345678", "7", "7", "1234567", "abc", ""] {
            let ctx = claim(Value::String(text.to_owned()));
            let error = visible(pool, descriptor(allow, &[]), &ctx)
                .await
                .expect_err("a string claim must never return rows");
            assert_eq!(
                sqlstate(&error).as_deref(),
                Some("42883"),
                "{predicate:?} claim {text:?}: {error}"
            );
        }

        let ctx = claim(Value::String("7".to_owned()));
        let preflight = authorize_record_action(
            &runtime,
            descriptor(allow, &[]),
            BigInt::new(7),
            allow,
            &[],
            &ctx,
            "update",
        )
        .await;
        assert!(
            preflight.is_err(),
            "{predicate:?}: the preflight must not authorize: {preflight:?}"
        );
    }
}
