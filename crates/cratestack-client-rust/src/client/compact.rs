//! Compact wire keys, client side (`docs/design/compact-wire-keys.md`,
//! proposal C). Opt-in: a client without
//! [`with_compact_keys`](CratestackClient::with_compact_keys) sends and
//! reads exactly what it always did.
//!
//! With it on, every call the client can name an op for sends
//! `Accept: <compact>, <plain>` and `Cratestack-Contract`, and a compact
//! response is turned back into named bytes before the normal decode. The
//! request body goes out compact only once a response for that op has come
//! back compact (a per-op flag held in memory), so an old server is never
//! sent a body it cannot read. `/rpc/batch` stays named.
//!
//! **Sealed (COSE) calls** are adaptive the same way: the compact types
//! lead `Cratestack-Payload-Accept`, the request is sealed under
//! `application/vnd.cratestack.compact+cbor` (or `+json`) only once the op
//! answered compact, and a compact sealed answer is transcoded after it is
//! opened. The server must opt the compact types in
//! (`EnvelopeLayerBuilder::payload_media_types`); against one that has not,
//! the answer is plain and the client stays named, and an unsigned `415`
//! (the compact request type refused) turns the op back to named for the
//! next call. That failed call is returned, not retried.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use cratestack_core::compact::{Direction, KeyNode, KeyTables};
use cratestack_core::{ContractSelector, OpContracts};
use reqwest::Method;

use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair};

/// Encoding pins kept; past this the oldest first attempt's pin is evicted, so a
/// prompt retry is covered and a long-lived instance keeps negotiating compact.
const MAX_REMEMBERED_KEYS: usize = 1024;

/// What a client opted into compact keys carries; shared by every clone.
pub(crate) struct CompactClient {
    pub(crate) tables: KeyTables,
    by_op: HashMap<String, usize>,
    /// Ops whose last 2xx response came back compact.
    compact_ops: Mutex<HashSet<String>>,
    /// `Idempotency-Key` -> whether its first attempt went out compact.
    idempotent: Mutex<Pins>,
}

impl CompactClient {
    pub(crate) fn new(mut tables: KeyTables, current: Option<OpContracts>) -> Self {
        if let Some(current) = current {
            // A server's tables also hold older accepted contracts; a
            // client speaks only its own.
            tables.ops.retain(|keys| {
                current
                    .iter()
                    .any(|(op, digest)| *op == keys.op && *digest == keys.digest)
            });
        }
        let by_op = tables
            .ops
            .iter()
            .enumerate()
            .map(|(index, keys)| (keys.op.clone(), index))
            .collect();
        Self {
            tables,
            by_op,
            compact_ops: Mutex::default(),
            idempotent: Mutex::default(),
        }
    }

    pub(crate) fn node(&self, index: usize, direction: Direction) -> &KeyNode {
        self.tables.node(&self.tables.ops[index], direction)
    }

    pub(crate) fn set_compact(&self, op: &str, on: bool) {
        let mut ops = lock(&self.compact_ops);
        if on {
            ops.insert(op.to_owned());
        } else {
            ops.remove(op);
        }
    }

    #[cfg(test)]
    pub(crate) fn is_compact(&self, op: &str) -> bool {
        lock(&self.compact_ops).contains(op)
    }

    fn decide(&self, op: &str, idempotency_key: Option<&str>) -> bool {
        let wanted = lock(&self.compact_ops).contains(op);
        let Some(key) = idempotency_key else {
            return wanted;
        };
        let mut pins = lock(&self.idempotent);
        if let Some(remembered) = pins.by_key.get(key) {
            return *remembered;
        }
        if pins.by_key.len() >= MAX_REMEMBERED_KEYS
            && let Some(oldest) = pins.order.pop_front()
        {
            pins.by_key.remove(&oldest);
        }
        pins.by_key.insert(key.to_owned(), wanted);
        pins.order.push_back(key.to_owned());
        wanted
    }
}

/// Idempotency-Key -> first attempt went compact, in first-attempt order.
#[derive(Default)]
pub(crate) struct Pins {
    pub(crate) by_key: HashMap<String, bool>,
    order: VecDeque<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The compact plan of one call.
pub(crate) struct CompactCall {
    pub(crate) state: Arc<CompactClient>,
    pub(crate) op: String,
    pub(crate) index: usize,
    pub(crate) send_compact: bool,
    pub(crate) selector: String,
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Turn compact wire keys on, with `tables` holding one entry per op
    /// (the client's own contracts). Off by default.
    #[must_use]
    pub fn with_compact_keys(mut self, tables: KeyTables) -> Self {
        self.compact = Some(Arc::new(CompactClient::new(tables, None)));
        self
    }

    /// Compact wire keys from the generated `KEY_TABLES` bytes, keeping the
    /// entries whose digest is the op's current one in `contracts` (the
    /// generated `OP_CONTRACTS`). Called by the generated
    /// `Client::with_compact_keys`.
    pub fn with_compact_key_bytes(
        mut self,
        tables: &[u8],
        contracts: OpContracts,
    ) -> Result<Self, ClientError> {
        let tables = KeyTables::from_bytes(tables)
            .map_err(|error| ClientError::BadInput(format!("invalid KEY_TABLES: {error}")))?;
        self.compact = Some(Arc::new(CompactClient::new(tables, Some(contracts))));
        Ok(self)
    }

    /// The plan for the call in flight, or `None` (compact off, no route named, or an op the tables lack such as `batch`).
    pub(crate) fn compact_call(
        &self,
        method: &Method,
        headers: &[HeaderPair<'_>],
        has_body: bool,
    ) -> Option<CompactCall> {
        let state = self.compact.as_ref()?;
        let template = self.compact_route.as_deref()?;
        let (op, index) = match state.by_op.get(template) {
            Some(index) => (template.to_owned(), *index),
            None => {
                let verb = if method == Method::HEAD {
                    "GET"
                } else {
                    method.as_str()
                };
                let op = format!("{verb} {template}");
                let index = *state.by_op.get(&op)?;
                (op, index)
            }
        };
        let key = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("idempotency-key"))
            .map(|(_, value)| *value);
        let send_compact = has_body && state.decide(&op, key);
        let selector = ContractSelector::of(&state.tables.ops[index].digest).to_header_value();
        Some(CompactCall {
            state: Arc::clone(state),
            op,
            index,
            send_compact,
            selector,
        })
    }
}

#[cfg(test)]
#[path = "compact_idempotency_tests.rs"]
mod idempotency_tests;
