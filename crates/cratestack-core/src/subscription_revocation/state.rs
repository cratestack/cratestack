//! The registry's state, behind one mutex: every operation is a short,
//! non-blocking map update, so it never holds the lock across an `await`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::watch;

use super::{RevocationKey, RevocationKeysFn, default_revocation_keys};
use crate::error::{StreamEndNotice, StreamEndReason};

pub(super) type EndSender = Arc<watch::Sender<Option<StreamEndNotice>>>;

pub(super) struct Tombstone {
    pub(super) reason: StreamEndReason,
    pub(super) until: SystemTime,
}

pub(super) struct State {
    pub(super) next_id: u64,
    pub(super) max_tombstones: usize,
    pub(super) now: fn() -> SystemTime,
    pub(super) keys: RevocationKeysFn,
    pub(super) watches: HashMap<u64, (EndSender, Vec<RevocationKey>)>,
    pub(super) by_key: HashMap<RevocationKey, HashSet<u64>>,
    pub(super) tombstones: HashMap<RevocationKey, Tombstone>,
}

fn end_watch(sender: &EndSender, end: &StreamEndNotice) -> bool {
    // The first reason wins: a stream already told why it ended keeps it.
    sender.send_if_modified(|current| {
        let first = current.is_none();
        if first {
            *current = Some(end.clone());
        }
        first
    })
}

impl State {
    pub(super) fn new(max_tombstones: usize) -> Self {
        Self {
            next_id: 0,
            max_tombstones,
            now: SystemTime::now,
            keys: default_revocation_keys,
            watches: HashMap::new(),
            by_key: HashMap::new(),
            tombstones: HashMap::new(),
        }
    }

    fn prune(&mut self) {
        let now = (self.now)();
        self.tombstones.retain(|_, tombstone| tombstone.until > now);
    }

    /// The reason `key` is tombstoned for, if it still is.
    pub(super) fn tombstoned(&mut self, keys: &[RevocationKey]) -> Option<StreamEndReason> {
        self.prune();
        keys.iter()
            .find_map(|key| self.tombstones.get(key).map(|tombstone| tombstone.reason))
    }

    pub(super) fn register(&mut self, keys: Vec<RevocationKey>) -> (u64, EndSender) {
        let id = self.next_id;
        self.next_id += 1;
        let sender: EndSender = Arc::new(watch::channel(None).0);
        for key in &keys {
            self.by_key.entry(key.clone()).or_default().insert(id);
        }
        self.watches.insert(id, (sender.clone(), keys));
        (id, sender)
    }

    pub(super) fn deregister(&mut self, id: u64) {
        let Some((_, keys)) = self.watches.remove(&id) else {
            return;
        };
        for key in keys {
            if let Some(ids) = self.by_key.get_mut(&key) {
                ids.remove(&id);
                if ids.is_empty() {
                    self.by_key.remove(&key);
                }
            }
        }
    }

    /// Ends every stream under `key`; the number ended.
    pub(super) fn end_key(&mut self, key: &RevocationKey, end: &StreamEndNotice) -> usize {
        let Some(ids) = self.by_key.get(key) else {
            return 0;
        };
        ids.iter()
            .filter_map(|id| self.watches.get(id))
            .filter(|(sender, _)| end_watch(sender, end))
            .count()
    }

    pub(super) fn end_all(&mut self, end: &StreamEndNotice) -> usize {
        self.watches
            .values()
            .filter(|(sender, _)| end_watch(sender, end))
            .count()
    }

    /// Keeps `key` refused until `until`. Over the cap, the tombstone that
    /// lapses soonest is dropped first: it protects the least.
    pub(super) fn tombstone(
        &mut self,
        key: RevocationKey,
        reason: StreamEndReason,
        until: SystemTime,
    ) {
        self.prune();
        if until <= (self.now)() {
            return;
        }
        let until = match self.tombstones.get(&key) {
            Some(existing) => existing.until.max(until),
            None => until,
        };
        if !self.tombstones.contains_key(&key) && self.tombstones.len() >= self.max_tombstones {
            let soonest = self
                .tombstones
                .iter()
                .min_by_key(|(_, tombstone)| tombstone.until)
                .map(|(key, _)| key.clone());
            match soonest {
                Some(soonest) => {
                    self.tombstones.remove(&soonest);
                }
                None => return,
            }
        }
        self.tombstones.insert(key, Tombstone { reason, until });
    }
}
