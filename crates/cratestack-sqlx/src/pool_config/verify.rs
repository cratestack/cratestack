//! [`PgPoolConfig::verify_settings`]: did the server really apply them?

use cratestack_core::CratestackError;

use super::PgPoolConfig;
use crate::cratestack_error_from_sqlx;
use crate::sqlx::{self, PgPool};

impl PgPoolConfig {
    /// Check that a connection from `pool` carries every configured session
    /// setting, and fail start-up if it does not.
    ///
    /// Catches a pooler (pgbouncer in transaction mode, for one) that drops
    /// startup options: the pool builds and works, just without the timeouts.
    ///
    /// Comparison is by value as the server renders it, not by spelling:
    /// Postgres turns `statement_timeout=30000` into `30s`, so the expected
    /// value is run through `set_config(.., is_local => true)` inside a
    /// transaction on the same connection and the two renderings are compared.
    /// The transaction is rolled back, leaving the connection as it was.
    ///
    /// A mismatch is `CratestackError::Unavailable` naming the setting with
    /// the expected and actual values. A configured value the server rejects
    /// is `BadRequest`. With no settings configured this succeeds without
    /// touching the pool.
    pub async fn verify_settings(&self, pool: &PgPool) -> Result<(), CratestackError> {
        self.connect_options()?; // refuse a config that could not have built `pool`
        if self.settings.is_empty() {
            return Ok(());
        }
        let mut tx = pool.begin().await.map_err(cratestack_error_from_sqlx)?;
        for (name, expected) in &self.settings {
            let actual: Option<String> = sqlx::query_scalar("SELECT current_setting($1, true)")
                .bind(name)
                .fetch_one(&mut *tx)
                .await
                .map_err(cratestack_error_from_sqlx)?;
            let canonical: String = sqlx::query_scalar("SELECT set_config($1, $2, true)")
                .bind(name)
                .bind(expected)
                .fetch_one(&mut *tx)
                .await
                .map_err(|error| match error {
                    sqlx::Error::Database(_) => CratestackError::BadRequest(format!(
                        "the server rejects `{expected}` as a value for session setting `{name}`"
                    )),
                    other => cratestack_error_from_sqlx(other),
                })?;
            let actual = actual.unwrap_or_default();
            if actual != canonical {
                return Err(CratestackError::Unavailable(format!(
                    "postgres session setting `{name}` is `{actual}` but `{canonical}` was \
                     requested; a connection pooler may be dropping startup options"
                )));
            }
        }
        tx.rollback().await.map_err(cratestack_error_from_sqlx)
    }
}
