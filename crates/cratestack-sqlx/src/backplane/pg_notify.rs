//! A backplane over Postgres `LISTEN`/`NOTIFY`: every replica sharing the
//! database receives every message, its own included
//! (`docs/design/subscriptions-websocket.md` §4.2, §4.3).
//!
//! One listener connection per process, opened by the first subscription and
//! kept for the life of the backplane (dropping the backplane closes it, idle
//! or not); local subscribers fan out from it. Model
//! events are notified by the writing transaction itself
//! (`enqueue_event_outbox`), so Postgres delivers them only if it commits, to
//! every listener, in commit order. A notification is a CBOR envelope
//! (`notify_payload`), or the 0.20.5 JSON one for a rolling deploy
//! ([`PgNotifyBackplane::with_json_envelope`]); text and byte payloads both
//! travel in it. When the listener's connection drops,
//! notifications sent meanwhile are lost: every local subscription gets
//! `Resync` and ends (§4.5).

use std::sync::{Arc, Weak};
use std::time::Duration;

use cratestack_core::backplane::LocalHub;
use cratestack_core::{BackplaneFuture, BackplaneReceiver, CratestackError, SubscriptionBackplane};
use tokio::sync::{OnceCell, watch};

use super::deliver::deliver;
use super::notify_payload::{Envelope, NOTIFY_CHANNEL, NOTIFY_LIMIT, encode_bytes, encode_text};
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;
use crate::sqlx::postgres::PgListener;

pub struct PgNotifyBackplane {
    pool: sqlx::PgPool,
    hub: Arc<LocalHub>,
    listening: OnceCell<()>,
    /// Dropped with the backplane, which ends the listener task: `try_recv`
    /// on an idle connection would otherwise wait forever.
    alive: watch::Sender<()>,
}

impl PgNotifyBackplane {
    /// The listener takes one connection from `pool`'s settings (not from the
    /// pool itself). It needs a session that keeps `LISTEN` state, so not a
    /// PgBouncer in transaction mode.
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            hub: Arc::default(),
            listening: OnceCell::new(),
            alive: watch::channel(()).0,
        }
    }

    /// Makes every notification this **process** writes use the legacy JSON
    /// envelope (0.20.5 and earlier) instead of the CBOR one, for a rolling
    /// deploy from those versions. A 0.20.5 replica cannot read CBOR: it
    /// skips the notification as unreadable. Every 0.20.6 replica reads both.
    ///
    /// Call it once at startup, before the first write. **It is a process
    /// switch, not a property of one backplane:** it covers every
    /// `PgNotifyBackplane` in the process, built before or after the call
    /// (another schema's default runtime included), and the writers that
    /// hold no backplane (a model event's write path, a topic's
    /// `publish_in_tx`). Writes made *before* the call went out as CBOR, and
    /// it cannot be undone in-process: to go back, redeploy without it.
    ///
    /// **Rollout.** Step 1: deploy 0.20.6 with this on every replica; they
    /// write what the old replicas read, and read both. Step 2, once no
    /// 0.20.5 replica is left: remove it and redeploy; they write CBOR.
    /// A byte payload goes as base64 in a field (`b`)
    /// that 0.20.5 readers ignore, so it reaches 0.20.6 replicas only, and
    /// its size limit is the base64 text's.
    pub fn use_json_envelope() {
        Envelope::make_default_json();
    }

    /// [`Self::use_json_envelope`] in a builder chain: process-wide, not `self`'s.
    #[must_use]
    pub fn with_json_envelope(self) -> Self {
        Self::use_json_envelope();
        self
    }

    async fn notify(&self, notification: String) -> Result<(), CratestackError> {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(NOTIFY_CHANNEL)
            .bind(notification)
            .execute(&self.pool)
            .await
            .map_err(cratestack_error_from_sqlx)?;
        Ok(())
    }

    async fn ensure_listening(&self) -> Result<(), CratestackError> {
        self.listening
            .get_or_try_init(|| async {
                let mut listener = PgListener::connect_with(&self.pool)
                    .await
                    .map_err(cratestack_error_from_sqlx)?;
                listener
                    .listen(NOTIFY_CHANNEL)
                    .await
                    .map_err(cratestack_error_from_sqlx)?;
                tokio::spawn(listen(
                    listener,
                    Arc::downgrade(&self.hub),
                    self.pool.clone(),
                    self.alive.subscribe(),
                ));
                Ok(())
            })
            .await
            .map(|_| ())
    }
}

impl SubscriptionBackplane for PgNotifyBackplane {
    fn publish<'a>(
        &'a self,
        channel: &'a str,
        payload: &'a str,
    ) -> BackplaneFuture<'a, Result<(), CratestackError>> {
        Box::pin(async move {
            let Some(notification) = encode_text(Envelope::current(), channel, payload, None)
            else {
                return Err(CratestackError::BadRequest(
                    "a backplane payload must stay under Postgres' 8000-byte NOTIFY limit"
                        .to_owned(),
                ));
            };
            self.notify(notification).await
        })
    }

    fn publish_bytes<'a>(
        &'a self,
        channel: &'a str,
        payload: &'a [u8],
    ) -> BackplaneFuture<'a, Result<(), CratestackError>> {
        Box::pin(async move {
            // No outbox row exists for a byte payload to refer to, so it
            // is refused whole, never truncated.
            let notification =
                encode_bytes(Envelope::current(), channel, payload).map_err(|size| {
                    CratestackError::BadRequest(format!(
                        "a backplane byte payload of {} bytes encodes to a {size}-byte NOTIFY, \
                     over Postgres' limit of {NOTIFY_LIMIT} bytes",
                        payload.len()
                    ))
                })?;
            self.notify(notification).await
        })
    }

    fn subscribe<'a>(
        &'a self,
        channel: &'a str,
    ) -> BackplaneFuture<'a, Result<BackplaneReceiver, CratestackError>> {
        Box::pin(async move {
            // Attach first, then make sure LISTEN is live: nothing delivered
            // after this returns can be missed.
            let receiver = self.hub.subscribe(channel);
            self.ensure_listening().await?;
            Ok(receiver)
        })
    }

    fn publishes_model_events_in_tx(&self) -> bool {
        true
    }
}

/// Runs until the backplane is dropped, then closes its connection.
/// `try_recv` reports a lost connection as `Ok(None)` and reconnects
/// (re-`LISTEN`ing) on the next call.
async fn listen(
    mut listener: PgListener,
    hub: Weak<LocalHub>,
    pool: sqlx::PgPool,
    mut alive: watch::Receiver<()>,
) {
    loop {
        let received = tokio::select! {
            received = listener.try_recv() => received,
            // `Err` once the backplane (the only sender) is dropped.
            _ = alive.changed() => return,
        };
        let Some(hub) = hub.upgrade() else { return };
        match received {
            Ok(Some(notification)) => deliver(&hub, &pool, notification.payload()).await,
            Ok(None) => {
                tracing::warn!(target: "cratestack", "backplane listener reconnecting; subscriptions resync");
                hub.resync_all();
            }
            Err(error) => {
                tracing::warn!(target: "cratestack", %error, "backplane listener failed; retrying");
                hub.resync_all();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
