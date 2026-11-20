//! The follower's gap paths, each driven alone: a backplane that says it may
//! have lost messages must end every watched stream under `EndAll`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};

use super::*;
use crate::backplane::{
    BackplaneFuture, BackplaneMessage, BackplaneReceiver, InProcessBackplane, SubscriptionBackplane,
};
use crate::error::CratestackError;

/// Hands out one scripted receiver per `subscribe`; once the script runs
/// out, receivers that never yield.
struct Scripted {
    receivers: Mutex<VecDeque<Vec<BackplaneMessage>>>,
    /// Whether a scripted receiver stays open (pends) after its messages.
    stay_open: bool,
}

impl Scripted {
    fn new(script: Vec<Vec<BackplaneMessage>>, stay_open: bool) -> Arc<Self> {
        Arc::new(Self {
            receivers: Mutex::new(script.into()),
            stay_open,
        })
    }
}

impl SubscriptionBackplane for Scripted {
    fn publish<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> BackplaneFuture<'a, Result<(), CratestackError>> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn subscribe<'a>(
        &'a self,
        _: &'a str,
    ) -> BackplaneFuture<'a, Result<BackplaneReceiver, CratestackError>> {
        let scripted = self.receivers.lock().unwrap().pop_front();
        let receiver: BackplaneReceiver = match scripted {
            Some(messages) if self.stay_open => {
                Box::pin(stream::iter(messages).chain(stream::pending()))
            }
            Some(messages) => Box::pin(stream::iter(messages)),
            None => Box::pin(stream::pending()),
        };
        Box::pin(std::future::ready(Ok(receiver)))
    }
}

fn key() -> RevocationKey {
    RevocationKey::Session("s".to_owned())
}

async fn ended_by(script: Vec<Vec<BackplaneMessage>>, stay_open: bool) -> Option<StreamEndReason> {
    let registry = StreamRevocations::default();
    let mut watch = registry.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(registry.follow(Scripted::new(script, stay_open)));
    let ended = tokio::time::timeout(Duration::from_millis(500), watch.ended())
        .await
        .ok()
        .map(|end| end.reason);
    follower.abort();
    ended
}

#[tokio::test]
async fn a_lagged_receiver_ends_every_stream() {
    let reason = ended_by(vec![vec![BackplaneMessage::Lagged]], true).await;
    assert_eq!(reason, Some(StreamEndReason::RevocationResync));
}

#[tokio::test]
async fn a_resync_ends_every_stream() {
    let reason = ended_by(vec![vec![BackplaneMessage::Resync]], true).await;
    assert_eq!(reason, Some(StreamEndReason::RevocationResync));
}

#[tokio::test]
async fn a_dropped_receiver_ends_every_stream() {
    let reason = ended_by(vec![vec![]], false).await;
    assert_eq!(reason, Some(StreamEndReason::RevocationResync));
}

#[tokio::test]
async fn a_healthy_receiver_ends_nothing() {
    assert_eq!(ended_by(vec![vec![]], true).await, None);
}

#[tokio::test]
async fn the_follower_ending_for_any_reason_fails_closed() {
    // A follower dropped mid-flight (aborted, or unwound by a panic) must
    // not leave streams trusted with nothing listening for revocations.
    let registry = StreamRevocations::default();
    let watch = registry.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(registry.follow(Arc::new(InProcessBackplane::default())));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(watch.try_ended().is_none());
    follower.abort();
    let _ = follower.await;
    assert_eq!(
        watch.try_ended().unwrap().reason,
        StreamEndReason::RevocationResync
    );
}
