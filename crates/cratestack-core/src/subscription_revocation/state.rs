//! The registry's state, behind one mutex: every operation is a short,
//! non-blocking map update, so it never holds the lock across an `await`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::watch;

use super::{RevocationKey, RevocationKeysFn, default_revocation_keys};
use crate::error::{StreamEndNotice, StreamEndReason};

pub(super) type EndSender = Arc<watch::Sender<Option<StreamEndNotice>>>;

pub(super) struct Tombstone {
    pub(super) reason: StreamEndReason,
    pub(super) until: SystemTime,
    /// Disambiguates tombstones lapsing at the same instant in `by_until`.
    seq: u64,
}

pub(super) struct State {
    pub(super) next_id: u64,
    pub(super) max_tombstones: usize,
    pub(super) now: fn() -> SystemTime,
    pub(super) keys: RevocationKeysFn,
    pub(super) watches: HashMap<u64, (EndSender, Vec<RevocationKey>)>,
    pub(super) by_key: HashMap<RevocationKey, HashSet<u64>>,
    pub(super) tombstones: HashMap<RevocationKey, Tombstone>,
    /// The same tombstones ordered by when they lapse, so pruning and
    /// eviction pop the front instead of scanning the map.
    by_until: BTreeMap<(SystemTime, u64), RevocationKey>,
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
            by_until: BTreeMap::new(),
        }
    }

    /// Drops the tombstones that have lapsed. Costs O(log n) per lapsed
    /// one; called when a tombstone is written, never on the open path.
    pub(super) fn prune(&mut self) {
        let now = (self.now)();
        while let Some(entry) = self.by_until.first_entry() {
            if entry.key().0 > now {
                break;
            }
            self.tombstones.remove(&entry.remove());
        }
    }

    /// The reason a key is tombstoned for, if any still is. A lapsed
    /// tombstone that has not been pruned yet is not one: lapse is checked
    /// here, so opening a stream never scans or mutates the maps.
    pub(super) fn tombstoned(&self, keys: &[RevocationKey]) -> Option<StreamEndReason> {
        let now = (self.now)();
        keys.iter().find_map(|key| {
            self.tombstones
                .get(key)
                .filter(|tombstone| tombstone.until > now)
                .map(|tombstone| tombstone.reason)
        })
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
        if reason.is_unavailable() {
            // Refusing an open with a reason the client retries without a
            // cap would reconnect every client of the principal until
            // `until`: end the live streams only.
            tracing::warn!(
                ?reason,
                "not keeping a tombstone for a resubscribe reason: it would loop reconnects"
            );
            return;
        }
        self.prune();
        if until <= (self.now)() {
            return;
        }
        let existing = self.tombstones.remove(&key);
        let until = match &existing {
            Some(existing) => {
                self.by_until.remove(&(existing.until, existing.seq));
                existing.until.max(until)
            }
            None => until,
        };
        if existing.is_none() && self.tombstones.len() >= self.max_tombstones {
            match self.by_until.pop_first() {
                Some((_, evicted)) => {
                    tracing::warn!(
                        max = self.max_tombstones,
                        "revocation tombstone cap reached, evicting the soonest to lapse; \
                         that principal's key is no longer refused at open"
                    );
                    self.tombstones.remove(&evicted);
                }
                None => return,
            }
        }
        let seq = self.next_id;
        self.next_id += 1;
        self.by_until.insert((until, seq), key.clone());
        self.tombstones
            .insert(key, Tombstone { reason, until, seq });
    }
}
