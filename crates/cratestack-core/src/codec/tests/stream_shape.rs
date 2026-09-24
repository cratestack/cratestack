//! The provisional stream traits (P1, cratestack#1008, finalises them) must
//! at least be object-safe, and able to express §6's cadence, mandatory
//! terminal checkpoint and `Incomplete`. A toy counter chain proves that
//! through `Box<dyn …>`, the way a router will hold them. No hashing and no
//! signatures: this checks the shape, not the cryptography.

use bytes::Bytes;

use crate::codec::{OpenedFrame, SealedItem, StreamEnd, StreamOpener, StreamSealer};
use crate::error::CratestackError;
use crate::rpc::RpcErrorBody;

#[derive(Default)]
struct ToySealer {
    sealed: u64,
    covered: u64,
}

impl ToySealer {
    fn checkpoint(&mut self) -> Bytes {
        self.covered = self.sealed;
        Bytes::from(format!("cp:{}", self.sealed))
    }
}

#[async_trait::async_trait]
impl StreamSealer for ToySealer {
    async fn seal_item(&mut self, item: Bytes) -> Result<SealedItem, CratestackError> {
        self.sealed += 1;
        let checkpoint = (self.sealed - self.covered == 2).then(|| self.checkpoint());
        Ok(SealedItem { item, checkpoint })
    }

    async fn checkpoint_now(&mut self) -> Result<Option<Bytes>, CratestackError> {
        Ok((self.sealed > self.covered).then(|| self.checkpoint()))
    }

    async fn finish(self: Box<Self>, end: StreamEnd) -> Result<Bytes, CratestackError> {
        Ok(match end {
            StreamEnd::Complete => Bytes::from_static(b"end"),
            StreamEnd::Failed(body) => Bytes::from(format!("err:{}", body.code)),
        })
    }
}

#[derive(Default)]
struct ToyOpener {
    items: u64,
    ended: bool,
}

#[async_trait::async_trait]
impl StreamOpener for ToyOpener {
    async fn open_frame(&mut self, frame: Bytes) -> Result<OpenedFrame, CratestackError> {
        let tamper = || CratestackError::Unauthorized("invalid stream".to_owned());
        if let Some(seq) = frame.strip_prefix(b"cp:") {
            let seq: u64 = std::str::from_utf8(seq)
                .map_err(|_| tamper())?
                .parse()
                .map_err(|_| tamper())?;
            return if seq == self.items {
                Ok(OpenedFrame::Checkpoint)
            } else {
                Err(tamper())
            };
        }
        if &frame[..] == b"end" {
            self.ended = true;
            return Ok(OpenedFrame::End);
        }
        self.items += 1;
        Ok(OpenedFrame::Item(frame))
    }

    fn finish(self: Box<Self>) -> Result<(), CratestackError> {
        if self.ended {
            Ok(())
        } else {
            Err(CratestackError::Unauthorized(
                "incomplete stream".to_owned(),
            ))
        }
    }
}

async fn seal_three(end: StreamEnd) -> Vec<Bytes> {
    let mut sealer: Box<dyn StreamSealer> = Box::new(ToySealer::default());
    let mut wire = Vec::new();
    for item in [&b"a"[..], b"b", b"c"] {
        let sealed = sealer
            .seal_item(Bytes::copy_from_slice(item))
            .await
            .unwrap();
        wire.push(sealed.item);
        wire.extend(sealed.checkpoint);
    }
    // The time-based cadence falls due with "c" uncovered…
    wire.extend(sealer.checkpoint_now().await.unwrap());
    // …and once covered there is nothing left to checkpoint.
    assert_eq!(sealer.checkpoint_now().await.unwrap(), None);
    wire.push(sealer.finish(end).await.unwrap());
    wire
}

#[tokio::test]
async fn sealer_emits_cadence_and_terminal_checkpoints() {
    let wire = seal_three(StreamEnd::Complete).await;
    let wire: Vec<&[u8]> = wire.iter().map(|b| &b[..]).collect();
    assert_eq!(wire, [&b"a"[..], b"b", b"cp:2", b"c", b"cp:3", b"end"]);

    let failed = StreamEnd::Failed(RpcErrorBody::from_cratestack(&CratestackError::Internal(
        "boom".to_owned(),
    )));
    let wire = seal_three(failed).await;
    assert_eq!(wire.last().map(|b| &b[..]), Some(&b"err:internal"[..]));
}

#[tokio::test]
async fn opener_accepts_a_complete_stream_and_flags_a_truncated_one() {
    let wire = seal_three(StreamEnd::Complete).await;

    let mut opener: Box<dyn StreamOpener> = Box::new(ToyOpener::default());
    for frame in wire.iter().cloned() {
        opener.open_frame(frame).await.unwrap();
    }
    opener.finish().expect("terminal checkpoint seen");

    // Cut before the terminal checkpoint: a clean close must not read as a
    // short, successful stream (§6 `Incomplete`).
    let mut opener: Box<dyn StreamOpener> = Box::new(ToyOpener::default());
    for frame in wire[..wire.len() - 1].iter().cloned() {
        opener.open_frame(frame).await.unwrap();
    }
    assert!(opener.finish().is_err());
}

#[tokio::test]
async fn opener_rejects_a_dropped_item_at_the_next_checkpoint() {
    let wire = seal_three(StreamEnd::Complete).await;
    let mut opener: Box<dyn StreamOpener> = Box::new(ToyOpener::default());
    opener.open_frame(wire[0].clone()).await.unwrap();
    // wire[1] ("b") dropped in transit.
    assert!(opener.open_frame(wire[2].clone()).await.is_err());
}
