//! Per-stream sealing and opening (ADR 0006 §6, `chain` mode).
//!
//! **Provisional: the shape is finalised in P1 (cratestack#1008,
//! cratestack#1009).** These are the smallest object-safe traits that can
//! carry §6's semantics, so that [`CratestackEnvelope::stream_sealer`] and
//! [`CratestackEnvelope::stream_opener`] have a real type to return in P0.
//! Nothing implements them yet, and [`NoEnvelope`](super::NoEnvelope)
//! returns `None` for both. Expect P1 to change them.
//!
//! They are `#[async_trait]` like core's other object-safe async traits
//! (`KeyProvider`, `NonceStore`): a checkpoint signature may come from a KMS
//! or HSM. The boxed future costs one allocation per call, and only on a
//! signed stream.
//!
//! [`CratestackEnvelope::stream_sealer`]: super::CratestackEnvelope::stream_sealer
//! [`CratestackEnvelope::stream_opener`]: super::CratestackEnvelope::stream_opener

use bytes::Bytes;

use crate::error::CratestackError;
use crate::rpc::RpcErrorBody;

/// What sealing one stream item produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedItem {
    /// The bytes to write for the item. In chain mode (the default) this is
    /// the input item, unchanged: items stay bare CBOR. In per-item mode
    /// (opt-in per op) it is the item's own COSE_Sign1.
    pub item: Bytes,
    /// A checkpoint (`#6.48901(COSE_Sign1)`) to write immediately after
    /// `item`, when the item/byte cadence is due (ADR 0006 Q3).
    pub checkpoint: Option<Bytes>,
}

/// How a stream ended. The terminal checkpoint that carries it is mandatory
/// (§6).
#[derive(Debug, Clone)]
pub enum StreamEnd {
    /// The producer finished: the terminal checkpoint carries `end: true`.
    Complete,
    /// The producer failed. The terminal checkpoint carries this body and,
    /// in signed mode, **replaces** the unsigned `Tag(48900, …)` sentinel,
    /// so a forged error is detectable too.
    Failed(RpcErrorBody),
}

/// Seals one outgoing stream (§6). Created per stream by
/// [`CratestackEnvelope::stream_sealer`](super::CratestackEnvelope::stream_sealer),
/// which seeds the running hash with `h₀ = SHA-256(external_aad)`.
///
/// Provisional; the shape is finalised in P1 (cratestack#1008).
#[async_trait::async_trait]
pub trait StreamSealer: Send + 'static {
    /// Feed one item's encoded bytes and return what to write.
    async fn seal_item(&mut self, item: Bytes) -> Result<SealedItem, CratestackError>;

    /// Emit a checkpoint now if any item is not yet covered by one. This is
    /// the hook for the time-based cadence (every 2 s; ADR 0006 Q3), which
    /// can fall due between items. `None` when there is nothing to cover.
    async fn checkpoint_now(&mut self) -> Result<Option<Bytes>, CratestackError>;

    /// The mandatory terminal checkpoint. It consumes the sealer, so a
    /// stream cannot be ended twice.
    async fn finish(self: Box<Self>, end: StreamEnd) -> Result<Bytes, CratestackError>;
}

/// What the opener made of one framed cbor-seq item.
///
/// `#[non_exhaustive]` because P1 (cratestack#1008) owns this shape.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OpenedFrame {
    /// A data item for `codec.decode`. In chain mode it is **not yet
    /// authenticated**. It becomes authenticated only when a later
    /// [`Checkpoint`](Self::Checkpoint) or [`End`](Self::End) verifies. The
    /// consumer's mode decides what to do until then (§6): `Verified`
    /// buffers the item, `Optimistic` releases it and rolls back on a
    /// failed checkpoint.
    Item(Bytes),
    /// A checkpoint verified. Every item since the previous one is
    /// authenticated.
    Checkpoint,
    /// The terminal checkpoint verified with `end: true`.
    End,
    /// The terminal checkpoint verified and carries the producer's error.
    Failed(RpcErrorBody),
}

/// Verifies one incoming stream (§6). Framing is not its job. The cbor-seq
/// chunk decoder splits the body into data items and hands each one here.
/// The opener classifies it (item, or `Tag(48901)` checkpoint) and checks
/// the chain.
///
/// Provisional; the shape is finalised in P1 (cratestack#1008,
/// cratestack#1009).
#[async_trait::async_trait]
pub trait StreamOpener: Send + 'static {
    /// Feed one framed data item. An `Err` is tampering (a reordered,
    /// dropped or injected item breaks the hash at the next checkpoint, and
    /// a forged checkpoint fails its signature), and the stream must be
    /// rejected.
    async fn open_frame(&mut self, frame: Bytes) -> Result<OpenedFrame, CratestackError>;

    /// The body ended. `Err` unless a terminal checkpoint verified: a
    /// stream cut short by a middlebox that closes cleanly is `Incomplete`,
    /// not a short successful stream.
    fn finish(self: Box<Self>) -> Result<(), CratestackError>;
}
