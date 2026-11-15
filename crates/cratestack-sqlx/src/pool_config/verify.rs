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
    /// the expected and actual values; a custom setting that is unset counts
    /// as a mismatch even when `""` was expected. `BadRequest` for a value
    /// the server rejects, a name it does not know, and a setting
    /// `set_config` cannot change on a live connection (context `backend`,
    /// `superuser-backend`: cannot be verified after connection start;
    /// `postmaster`, `sighup`: cannot be a session setting at all). With no
    /// settings configured this succeeds without touching the pool.
    pub async fn verify_settings(&self, pool: &PgPool) -> Result<(), CratestackError> {
        self.validate()?; // refuse a config that could not have built `pool`
        if self.settings.is_empty() {
            return Ok(());
        }
        let mut tx = pool.begin().await.map_err(cratestack_error_from_sqlx)?;
        for (name, expected) in &self.settings {
            let actual: Option<String> = sqlx::query_scalar("SELECT current_setting($1, true)")
                .bind(name)
                .fetch_optional(&mut *tx)
                .await
                .map_err(cratestack_error_from_sqlx)?
                .flatten();
            let context: Option<String> =
                sqlx::query_scalar("SELECT context FROM pg_settings WHERE name = $1")
                    .bind(name)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(cratestack_error_from_sqlx)?;
            match context.as_deref() {
                None if !name.contains('.') => {
                    return Err(CratestackError::BadRequest(format!(
                        "unknown setting `{name}`"
                    )));
                }
                Some("backend" | "superuser-backend") => {
                    return Err(CratestackError::BadRequest(format!(
                        "`{name}` cannot be verified after connection start"
                    )));
                }
                Some("postmaster" | "sighup") => {
                    return Err(CratestackError::BadRequest(format!(
                        "`{name}` cannot be a session setting"
                    )));
                }
                _ => {}
            }
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
            if actual.as_deref() != Some(canonical.as_str()) {
                return Err(CratestackError::Unavailable(format!(
                    "the server reports {} for `{name}`, expected `{canonical}` (a pooler \
                     dropping startup options, or a server/role/database-level override, \
                     can cause this)",
                    actual.map_or("<unset>".to_owned(), |a| format!("`{a}`")),
                )));
            }
        }
        tx.rollback().await.map_err(cratestack_error_from_sqlx)
    }
}
