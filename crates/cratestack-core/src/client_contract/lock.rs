//! The contract lock: a committed, content-addressed record of the op
//! contracts older clients in the field were built against (cratestack#1123,
//! EXT-14), so a server can keep accepting them while they stay
//! wire-compatible with the current contract.
//!
//! ```json
//! {
//!   "format": 1,
//!   "domain": "cratestack/op-contract/v1",
//!   "contracts": { "<op digest, hex>": { "...canonical op contract..." } },
//!   "generations": [
//!     { "client_contract": "<hex>", "locked_at": "2026-10-02", "note": "store 1.4.7",
//!       "ops": { "procedure.placeOrder": "<op digest, hex>" } }
//!   ]
//! }
//! ```
//!
//! `contracts` holds each distinct contract once, under its own digest, so an
//! op that did not change between two generations costs nothing the second
//! time. `generations` is chronological (oldest first, so a new lock appends
//! and a diff stays small) and records which digest each op had. The macro
//! recomputes every stored contract's digest, so a hand-edited lock cannot
//! smuggle a digest in, and classifies every entry against the current
//! contract ([`classify`](super::classify)): an incompatible one is a
//! compile error.
//!
//! No clock is read here: `locked_at` is whatever the caller passes, which
//! keeps builds reproducible. Dates are for pruning and for people.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::canonical_json::canonical_bytes;
use super::{OP_CONTRACT_DOMAIN, digest_hex, digest_of};

/// The lock file format this build reads and writes.
pub const LOCK_FORMAT: u32 = 1;

/// One locked moment: the client contract digest and each op's digest then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    /// Hex of `client_contract_digest` when this generation was locked.
    pub client_contract: String,
    /// A date (`YYYY-MM-DD`), as given when it was locked.
    pub locked_at: String,
    /// Free text: which store build or release carries it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Op key to op digest (hex), a key of `contracts`.
    pub ops: BTreeMap<String, String>,
}

/// A parsed, integrity-checked lock file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractLock {
    /// [`LOCK_FORMAT`].
    pub format: u32,
    /// The op-contract domain tag the digests were taken under.
    pub domain: String,
    /// Op digest (hex) to that op's canonical contract.
    pub contracts: BTreeMap<String, Value>,
    /// Oldest first.
    pub generations: Vec<Generation>,
}

/// Why a lock could not be read or used.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Not JSON of the lock's shape.
    #[error("the contract lock is not valid: {0}")]
    Parse(#[from] serde_json::Error),
    /// A format this build does not know.
    #[error("the contract lock has format {0}, this build reads format {LOCK_FORMAT}")]
    Format(u32),
    /// Digests taken under another derivation.
    #[error("the contract lock was made under domain {0:?}, not the current op-contract domain")]
    Domain(String),
    /// A stored contract does not hash to the digest it is filed under.
    #[error("the contract filed under {claimed} hashes to {actual}: the lock was edited by hand")]
    Integrity {
        /// The key in `contracts`.
        claimed: String,
        /// The digest of its content.
        actual: String,
    },
    /// A generation names a digest `contracts` does not hold.
    #[error("generation {generation} lists `{op}` under {digest}, which `contracts` lacks")]
    Dangling {
        /// The generation's client contract digest.
        generation: String,
        /// The op key.
        op: String,
        /// The missing digest.
        digest: String,
    },
    /// A digest that is not 64 lowercase hex digits.
    #[error("`{0}` is not a 64-digit lowercase hex digest")]
    NotHex(String),
    /// `prune --generation` matched no generation, or several.
    #[error("{0}")]
    Generation(String),
    /// Locked contracts the current one broke.
    #[error("{}", join_incompatible(.0))]
    Incompatible(Vec<super::lock_ops::Incompatible>),
}

fn join_incompatible(entries: &[super::lock_ops::Incompatible]) -> String {
    let lines: Vec<String> = entries.iter().map(ToString::to_string).collect();
    lines.join("\n")
}

pub(super) fn is_hex_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl ContractLock {
    /// A lock with no generations.
    pub fn new() -> Self {
        Self {
            format: LOCK_FORMAT,
            domain: String::from_utf8_lossy(OP_CONTRACT_DOMAIN)
                .trim_end_matches('\0')
                .to_owned(),
            contracts: BTreeMap::new(),
            generations: Vec::new(),
        }
    }

    /// Parse and integrity-check a lock file: format and domain, every
    /// stored contract hashing to its key, every generation's digests
    /// present. It does not judge compatibility (see
    /// [`ContractLock::accepted`]).
    pub fn parse(text: &str) -> Result<Self, LockError> {
        let lock: Self = serde_json::from_str(text)?;
        if lock.format != LOCK_FORMAT {
            return Err(LockError::Format(lock.format));
        }
        if lock.domain != Self::new().domain {
            return Err(LockError::Domain(lock.domain));
        }
        for (claimed, contract) in &lock.contracts {
            if !is_hex_digest(claimed) {
                return Err(LockError::NotHex(claimed.clone()));
            }
            let actual = digest_hex(&digest_of(&canonical_bytes(contract)));
            if &actual != claimed {
                return Err(LockError::Integrity {
                    claimed: claimed.clone(),
                    actual,
                });
            }
        }
        for generation in &lock.generations {
            if !is_hex_digest(&generation.client_contract) {
                return Err(LockError::NotHex(generation.client_contract.clone()));
            }
            for (op, digest) in &generation.ops {
                if !lock.contracts.contains_key(digest) {
                    return Err(LockError::Dangling {
                        generation: generation.client_contract.clone(),
                        op: op.clone(),
                        digest: digest.clone(),
                    });
                }
            }
        }
        Ok(lock)
    }

    /// The file's text: pretty JSON ending in a newline.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a lock always serializes");
        text.push('\n');
        text
    }
}

impl Default for ContractLock {
    fn default() -> Self {
        Self::new()
    }
}
