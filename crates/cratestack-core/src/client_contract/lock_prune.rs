//! Dropping history from a [`ContractLock`]: the lock keeps an entry as long
//! as the deployer does, and these are the deployer's knives. A contract no
//! generation references any more is dropped with it.

use super::lock::{ContractLock, LockError};

/// What a prune removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pruned {
    /// Whole generations removed.
    pub generations: usize,
    /// Per-op entries removed (whole generations' entries included).
    pub entries: usize,
    /// Stored contracts no generation references any more.
    pub contracts: usize,
}

impl ContractLock {
    /// Stop accepting older clients of one op: remove its entry from every
    /// generation. This is how a breaking change to an op is shipped on
    /// purpose.
    pub fn prune_op(&mut self, key: &str) -> Pruned {
        let entries = self
            .generations
            .iter_mut()
            .filter_map(|g| g.ops.remove(key))
            .count();
        self.settle(entries, 0)
    }

    /// Remove the generations locked before `date` (`YYYY-MM-DD`, compared
    /// as text).
    pub fn prune_before(&mut self, date: &str) -> Pruned {
        self.retain_generations(|g| g.locked_at.as_str() >= date)
    }

    /// Keep only the newest `keep` generations.
    pub fn prune_keep(&mut self, keep: usize) -> Pruned {
        let drop = self.generations.len().saturating_sub(keep);
        let mut index = 0;
        self.retain_generations(|_| {
            index += 1;
            index > drop
        })
    }

    /// Remove the one generation whose client contract digest starts with
    /// `prefix` (at least 8 hex digits).
    pub fn prune_generation(&mut self, prefix: &str) -> Result<Pruned, LockError> {
        let matching: Vec<&str> = self
            .generations
            .iter()
            .map(|g| g.client_contract.as_str())
            .filter(|hex| prefix.len() >= 8 && hex.starts_with(prefix))
            .collect();
        match matching.as_slice() {
            [one] => {
                let one = (*one).to_owned();
                Ok(self.retain_generations(|g| g.client_contract != one))
            }
            [] => Err(LockError::Generation(format!(
                "no generation starts with `{prefix}` (give at least 8 digits)"
            ))),
            _ => Err(LockError::Generation(format!(
                "`{prefix}` starts several generations; give more digits"
            ))),
        }
    }

    fn retain_generations(&mut self, mut keep: impl FnMut(&super::Generation) -> bool) -> Pruned {
        let (mut generations, mut entries) = (0, 0);
        self.generations.retain(|g| {
            let kept = keep(g);
            if !kept {
                generations += 1;
                entries += g.ops.len();
            }
            kept
        });
        self.settle(entries, generations)
    }

    /// Drop generations left with no ops and contracts nothing references.
    fn settle(&mut self, entries: usize, generations: usize) -> Pruned {
        let before = self.generations.len();
        self.generations.retain(|g| !g.ops.is_empty());
        let generations = generations + (before - self.generations.len());
        let referenced: std::collections::BTreeSet<&String> = self
            .generations
            .iter()
            .flat_map(|g| g.ops.values())
            .collect();
        let stored = self.contracts.len();
        self.contracts.retain(|hex, _| referenced.contains(hex));
        Pruned {
            generations,
            entries,
            contracts: stored - self.contracts.len(),
        }
    }
}
