//! A backplane for a single replica: published messages reach this process's
//! subscribers only. Model events are published after commit, from the
//! runtime's outbox drain.

use std::sync::Arc;

use cratestack_core::{BackplaneFuture, BackplaneReceiver, CratestackError, SubscriptionBackplane};

use super::LocalHub;

#[derive(Default)]
pub struct InProcessBackplane {
    hub: LocalHub,
}

impl InProcessBackplane {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SubscriptionBackplane for InProcessBackplane {
    fn publish<'a>(
        &'a self,
        channel: &'a str,
        payload: &'a str,
    ) -> BackplaneFuture<'a, Result<(), CratestackError>> {
        self.hub.deliver(channel, Arc::from(payload));
        Box::pin(std::future::ready(Ok(())))
    }

    fn subscribe<'a>(
        &'a self,
        channel: &'a str,
    ) -> BackplaneFuture<'a, Result<BackplaneReceiver, CratestackError>> {
        Box::pin(std::future::ready(Ok(self.hub.subscribe(channel))))
    }
}
