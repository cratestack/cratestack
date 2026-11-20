//! Revocations across replicas over `PgNotifyBackplane` (cratestack#1381):
//! two registries, each with its own pool and listener, share one database.
//! A revoke published on A ends a stream on B; a tombstone crosses too; and a
//! listener that reconnects (so a revocation may have been lost) ends every
//! watched stream with `revocation_resync` unless the follower is told to
//! ignore it.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha"). A skip reports `0.00s`.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    PgNotifyBackplane, ResyncPolicy, RevocationKey, RevocationWatch, StreamEndNotice,
    StreamEndReason, StreamRevocations, SubscriptionBackplane,
};

mod support;

use support::pg;

fn key() -> RevocationKey {
    RevocationKey::Session("sid-replicas".to_owned())
}

struct Replica {
    revocations: StreamRevocations,
    backplane: Arc<PgNotifyBackplane>,
    follower: tokio::task::JoinHandle<Result<(), cratestack::CratestackError>>,
}

impl Replica {
    async fn start(url: &str, policy: ResyncPolicy) -> Self {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(url)
            .await
            .expect("a replica's own pool");
        let backplane = Arc::new(PgNotifyBackplane::new(pool));
        let revocations = StreamRevocations::default();
        let follower = tokio::spawn(revocations.follow_with(backplane.clone(), policy));
        Self {
            revocations,
            backplane,
            follower,
        }
    }

    fn backplane(&self) -> &dyn SubscriptionBackplane {
        &*self.backplane
    }
}

/// Publishes from `from` until `watch` ends (the follower on the other side
/// subscribes on its first poll, so the first few may not be heard).
async fn publish_until_ended(
    from: &Replica,
    reason: StreamEndReason,
    until: Option<SystemTime>,
    watch: &mut RevocationWatch,
) -> StreamEndNotice {
    for _ in 0..100 {
        from.revocations
            .publish(from.backplane(), key(), reason, until)
            .await
            .expect("publish");
        if let Ok(end) = tokio::time::timeout(Duration::from_millis(100), watch.ended()).await {
            return end;
        }
    }
    panic!("the revocation never reached the other replica");
}

#[tokio::test]
async fn a_revoke_published_on_one_replica_ends_a_stream_on_another() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let (a, b) = (
        Replica::start(&test_pg.url, ResyncPolicy::EndAll).await,
        Replica::start(&test_pg.url, ResyncPolicy::EndAll).await,
    );
    let mut watch = b.revocations.watch(vec![key()]).unwrap();

    let end = publish_until_ended(&a, StreamEndReason::SessionEnded, None, &mut watch).await;
    assert_eq!(end.reason, StreamEndReason::SessionEnded);
    a.follower.abort();
    b.follower.abort();
}

#[tokio::test]
async fn a_published_tombstone_refuses_a_later_open_on_another_replica() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let (a, b) = (
        Replica::start(&test_pg.url, ResyncPolicy::EndAll).await,
        Replica::start(&test_pg.url, ResyncPolicy::EndAll).await,
    );
    let mut probe = b.revocations.watch(vec![key()]).unwrap();
    let until = SystemTime::now() + Duration::from_secs(60);

    publish_until_ended(&a, StreamEndReason::DeviceRevoked, Some(until), &mut probe).await;
    let refused = b.revocations.watch(vec![key()]).unwrap_err();
    assert_eq!(refused.reason, StreamEndReason::DeviceRevoked);
    a.follower.abort();
    b.follower.abort();
}

/// Kills every backplane listener connection: each follower's `LISTEN` is
/// lost, and with it whatever was published meanwhile.
async fn drop_listeners(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE query = 'LISTEN \"cratestack_events\"' AND pid <> pg_backend_pid()",
    )
    .execute(pool)
    .await
    .expect("terminate listeners");
}

#[tokio::test]
async fn a_listener_that_reconnects_ends_every_stream_with_revocation_resync() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let b = Replica::start(&test_pg.url, ResyncPolicy::EndAll).await;
    let mut watch = b.revocations.watch(vec![key()]).unwrap();
    // The follower has subscribed once its listener is up.
    tokio::time::sleep(Duration::from_millis(500)).await;

    drop_listeners(&test_pg.pool).await;
    let end = tokio::time::timeout(Duration::from_secs(10), watch.ended())
        .await
        .expect("the lost listener ended the stream");
    assert_eq!(end.reason, StreamEndReason::RevocationResync);
    b.follower.abort();
}

#[tokio::test]
async fn ignore_keeps_streams_across_a_reconnect_and_still_hears_later_revocations() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let (a, b) = (
        Replica::start(&test_pg.url, ResyncPolicy::EndAll).await,
        Replica::start(&test_pg.url, ResyncPolicy::Ignore).await,
    );
    let mut watch = b.revocations.watch(vec![key()]).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    drop_listeners(&test_pg.pool).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        watch.try_ended().is_none(),
        "ignored: the stream is untouched"
    );

    // The listener came back on its own: a revoke now still arrives, and it
    // is the revoke's reason, not a resync.
    let end = publish_until_ended(&a, StreamEndReason::PrincipalSuspended, None, &mut watch).await;
    assert_eq!(end.reason, StreamEndReason::PrincipalSuspended);
    a.follower.abort();
    b.follower.abort();
}
