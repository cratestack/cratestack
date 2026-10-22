//! Client registration and the principal binding check.
//!
//! A client id is issued by the server and bound to the principal that
//! registered it. Every later request re-checks that binding, so a stolen
//! or guessed client id is useless to another principal: it can neither
//! advance the victim's mutation sequence nor read the victim's acks.

use cratestack_core::CratestackContext;
use cratestack_sqlx::sqlx::{self, PgConnection};
use uuid::Uuid;

use super::{SyncError, SyncPrincipal, SyncServer, internal};
use crate::protocol::{RegisterRequest, RegisterResponse, SyncErrorCode};

const MAX_LABEL_CHARS: usize = 80;

impl SyncServer {
    pub async fn register(
        &self,
        ctx: &CratestackContext,
        request: RegisterRequest,
    ) -> Result<RegisterResponse, SyncError> {
        let principal = self.principal(ctx)?;
        self.admit("register", &principal.id).await?;
        let label = request
            .label
            .map(|label| label.chars().take(MAX_LABEL_CHARS).collect::<String>());

        let mut tx = self.pool.begin().await.map_err(internal)?;
        // Serialize registrations per principal so the cap below holds under
        // concurrency (two racing registrations cannot both see count < max).
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('cratestack_sync_register:' || $1))")
            .bind(&principal.id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        let held: i64 =
            sqlx::query_scalar("SELECT count(*) FROM cratestack_sync_clients WHERE principal = $1")
                .bind(&principal.id)
                .fetch_one(&mut *tx)
                .await
                .map_err(internal)?;
        if held >= self.policy.max_clients_per_principal {
            return Err(SyncError::new(
                SyncErrorCode::TooManyClients,
                format!(
                    "principal already holds {held} sync clients (max {})",
                    self.policy.max_clients_per_principal
                ),
            ));
        }
        let client_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO cratestack_sync_clients (client_id, principal, label) VALUES ($1, $2, $3)",
        )
        .bind(client_id)
        .bind(&principal.id)
        .bind(label)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        Ok(RegisterResponse {
            client_id,
            mutation_ttl_secs: self.policy.mutation_ttl_secs,
        })
    }
}

/// Load a client row for `principal`, refreshing its TTL lease.
/// `for_update` takes the row lock push needs to serialize seq checks.
pub(crate) async fn authorize(
    conn: &mut PgConnection,
    principal: &SyncPrincipal,
    client_id: Uuid,
    for_update: bool,
) -> Result<u64, SyncError> {
    let sql = if for_update {
        "SELECT principal, last_mutation_seq FROM cratestack_sync_clients \
         WHERE client_id = $1 FOR UPDATE"
    } else {
        "SELECT principal, last_mutation_seq FROM cratestack_sync_clients WHERE client_id = $1"
    };
    let row: Option<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(client_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(internal)?;
    let Some((owner, last_seq)) = row else {
        return Err(SyncError::new(
            SyncErrorCode::ClientExpired,
            "unknown or expired sync client; register again",
        ));
    };
    if owner != principal.id {
        // Deliberately does not say whose client it is.
        return Err(SyncError::new(
            SyncErrorCode::ClientMismatch,
            "sync client is registered to another principal",
        ));
    }
    sqlx::query("UPDATE cratestack_sync_clients SET last_seen_at = now() WHERE client_id = $1")
        .bind(client_id)
        .execute(&mut *conn)
        .await
        .map_err(internal)?;
    Ok(u64::try_from(last_seq).unwrap_or(0))
}
