//! A backplane over Postgres `LISTEN`/`NOTIFY`: every replica sharing the
//! database receives every message, its own included
//! (`docs/design/subscriptions-websocket.md` §4.2, §4.3).
//!
//! One listener connection per process, opened by the first subscription and
//! kept for the life of the backplane (dropping the backplane closes it, idle
//! or not); local subscribers fan out from it. Model
//! events are notified by the writing transaction itself
//! (`enqueue_event_outbox`), so Postgres delivers them only if it commits, to
//! every listener, in commit order. When the listener's connection drops,
//! notifications sent meanwhile are lost: every local subscription gets
//! `Resync` and ends (§4.5).

use std::sync::{Arc, Weak};
use std::time::Duration;

use cratestack_core::{BackplaneFuture, BackplaneReceiver, CratestackError, SubscriptionBackplane};
use tokio::sync::{OnceCell, watch};

use super::LocalHub;
use super::notify_payload::{NOTIFY_CHANNEL, Notification, encode};
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
            let Some(notification) = encode(channel, payload, None) else {
                return Err(CratestackError::BadRequest(
                    "a backplane payload must stay under Postgres' 8000-byte NOTIFY limit"
                        .to_owned(),
                ));
            };
            sqlx::query("SELECT pg_notify($1, $2)")
                .bind(NOTIFY_CHANNEL)
                .bind(notification)
                .execute(&self.pool)
                .await
                .map_err(cratestack_error_from_sqlx)?;
            Ok(())
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

async fn deliver(hub: &LocalHub, pool: &sqlx::PgPool, raw: &str) {
    let Ok(notification) = serde_json::from_str::<Notification<'_>>(raw) else {
        tracing::warn!(target: "cratestack", "unreadable backplane notification ignored");
        return;
    };
    let payload = match (notification.payload, notification.event_id) {
        (Some(payload), _) => Arc::from(payload.as_ref()),
        (None, Some(event_id)) => {
            match crate::descriptor::outbox_envelope_json(pool, event_id).await {
                Ok(payload) => Arc::from(payload),
                Err(error) => {
                    // The event is lost for this replica: say so rather than skip it.
                    tracing::warn!(target: "cratestack", %error, "large model event unreadable; its channel resyncs");
                    hub.resync(&notification.channel);
                    return;
                }
            }
        }
        (None, None) => return,
    };
    hub.deliver(&notification.channel, payload);
}
