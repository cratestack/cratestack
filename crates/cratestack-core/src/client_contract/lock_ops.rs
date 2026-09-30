//! What a [`ContractLock`] does against a schema: lock its current
//! contracts, judge the locked ones, and build the accepted table.

use std::collections::BTreeSet;

use serde_json::Value;

use super::build::canonical;
use super::lock::{ContractLock, Generation, LockError, is_hex_digest};
use super::ops::ops;
use super::{bound_contracts, classify, client_contract_digest, digest_hex, digest_of};
use crate::schema::Schema;

/// A locked contract the current one no longer stays compatible with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incompatible {
    /// The op key.
    pub op: String,
    /// Hex of the locked contract's digest.
    pub digest: String,
    /// What changed, from the classifier.
    pub reasons: Vec<String>,
}

impl std::fmt::Display for Incompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "op `{}`: the locked contract {} is not compatible with the current one ({}); \
             `cratestack contract prune --op {}` stops accepting that op's older clients, \
             which is the deliberate way to ship this change",
            self.op,
            &self.digest[..self.digest.len().min(12)],
            self.reasons.join("; "),
            self.op
        )
    }
}

/// Every current op: key, digest and canonical contract.
fn current(schema: &Schema) -> Vec<(String, [u8; 32], Value)> {
    ops(schema)
        .iter()
        .map(|op| {
            let bytes = canonical(schema, op);
            let value = serde_json::from_slice(&bytes).expect("canonical JSON parses");
            (op.key.clone(), digest_of(&bytes), value)
        })
        .collect()
}

fn digest_from_hex(hex: &str) -> Option<[u8; 32]> {
    if !is_hex_digest(hex) {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

impl ContractLock {
    /// Whether the schema's current contract is a recorded generation.
    pub fn is_locked(&self, schema: &Schema) -> bool {
        let client = digest_hex(&client_contract_digest(schema));
        self.generations.iter().any(|g| g.client_contract == client)
    }

    /// Record the schema's current contracts as a new generation. Returns
    /// `false`, changing nothing, when that client contract is already a
    /// generation (locking is idempotent).
    pub fn lock_generation(&mut self, schema: &Schema, locked_at: &str, note: &str) -> bool {
        if self.is_locked(schema) {
            return false;
        }
        let mut generation = Generation {
            client_contract: digest_hex(&client_contract_digest(schema)),
            locked_at: locked_at.to_owned(),
            note: note.to_owned(),
            ops: Default::default(),
        };
        for (key, digest, value) in current(schema) {
            let hex = digest_hex(&digest);
            self.contracts.entry(hex.clone()).or_insert(value);
            generation.ops.insert(key, hex);
        }
        self.generations.push(generation);
        true
    }

    /// Every locked contract of a still-existing op whose digest is not the
    /// current one and which the current contract is not compatible with.
    pub fn incompatible(&self, schema: &Schema) -> Vec<Incompatible> {
        let current = current(schema);
        let mut seen = BTreeSet::new();
        let mut broken = Vec::new();
        for generation in self.generations.iter().rev() {
            for (op, hex) in &generation.ops {
                let Some((_, digest, now)) = current.iter().find(|(key, ..)| key == op) else {
                    continue;
                };
                if digest_hex(digest) == *hex || !seen.insert((op.as_str(), hex.as_str())) {
                    continue;
                }
                let Some(old) = self.contracts.get(hex) else {
                    continue;
                };
                if let super::Verdict::Breaking(reasons) = classify(old, now) {
                    broken.push(Incompatible {
                        op: op.clone(),
                        digest: hex.clone(),
                        reasons,
                    });
                }
            }
        }
        broken
    }

    /// The server's accepted table: per op key (and `batch`), the current
    /// digest, then every locked digest of that op newest first. Fails with
    /// [`LockError::Incompatible`] when any locked contract of an existing
    /// op is not compatible with the current one, so an incompatible entry
    /// is never accepted silently.
    pub fn accepted(&self, schema: &Schema) -> Result<Vec<(String, Vec<[u8; 32]>)>, LockError> {
        let broken = self.incompatible(schema);
        if !broken.is_empty() {
            return Err(LockError::Incompatible(broken));
        }
        let mut table: Vec<(String, Vec<[u8; 32]>)> = bound_contracts(schema)
            .into_iter()
            .map(|(key, digest)| (key, vec![digest]))
            .collect();
        let op_keys: BTreeSet<String> = ops(schema).into_iter().map(|op| op.key).collect();
        for generation in self.generations.iter().rev() {
            for (op, hex) in &generation.ops {
                let Some(digest) = digest_from_hex(hex) else {
                    continue;
                };
                if !op_keys.contains(op) {
                    continue;
                }
                if let Some((_, digests)) = table.iter_mut().find(|(key, _)| key == op)
                    && !digests.contains(&digest)
                {
                    digests.push(digest);
                }
            }
        }
        Ok(table)
    }
}
