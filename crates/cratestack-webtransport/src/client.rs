//! The native client (feature `client`, design §6a.2): one session, its
//! control stream kept open for the session's life, and one bidirectional
//! stream per subscription. A subscription's `Request` is its stream's only
//! frame; dropping a [`Subscription`] stops its stream, which is the cancel.
//!
//! ```no_run
//! # async fn run() -> Result<(), cratestack_webtransport::ClientFailure> {
//! use cratestack_webtransport::{ServerTrust, WebTransportClient};
//! let url = "https://api.example.com:4433/rpc/wt".parse().unwrap();
//! let headers = [("authorization".to_owned(), "Bearer t0k3n".to_owned())].into();
//! let client = WebTransportClient::connect(&url, &ServerTrust::NativeRoots, headers).await?;
//! let mut notes = client.subscribe("model.Note.subscribe", serde_json::Value::Null).await?;
//! while let Some(item) = notes.next().await {
//!     let event = item?;
//! }
//! # Ok(()) }
//! ```

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use cratestack_core::rpc::RpcErrorBody;
use tokio::sync::Mutex;

use crate::codec::WireCodec;
use crate::frame::{ClientFrame, ServerFrame};
use crate::reader::{FrameReader, ReadFailure};
use crate::transport::client::{ServerTrust, connect};
use crate::transport::{SendHalf, Session};

mod bodies;
use bodies::{closed_body, going_away};

/// Frames larger than this are refused, as on the server.
const MAX_FRAME_BYTES: usize = 1 << 20;
/// How long the server has to answer `Init` or a `Request`.
const READY_WITHIN: Duration = Duration::from_secs(10);

/// Why a session or a subscription did not open, or ended.
#[derive(Debug)]
pub enum ClientFailure {
    /// No session: the network, UDP blocked, the handshake, the certificate.
    /// A caller with another binding falls back on this one only.
    Unreachable(String),
    /// The server answered with an error: a refused `Init` or `Request`, or
    /// a subscription's terminal `err`. Never a reason to fall back.
    Refused(RpcErrorBody),
}

impl std::fmt::Display for ClientFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(reason) => write!(f, "WebTransport unreachable: {reason}"),
            Self::Refused(body) => write!(f, "{}: {}", body.code, body.message),
        }
    }
}

impl std::error::Error for ClientFailure {}

fn unreachable(reason: impl std::fmt::Display) -> ClientFailure {
    ClientFailure::Unreachable(reason.to_string())
}

/// One authenticated session. Subscriptions opened on it share it.
pub struct WebTransportClient {
    session: Session,
    next_id: AtomicU64,
    /// Kept open: the server ends the session when its control stream ends.
    _control: Mutex<(SendHalf, FrameReader)>,
}

impl std::fmt::Debug for WebTransportClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransportClient").finish_non_exhaustive()
    }
}

impl WebTransportClient {
    /// CONNECTs to `url` (`https://host:port/rpc/wt`, or `/$wt` for a REST
    /// schema), sends `Init` with `headers` for the server's `AuthProvider`,
    /// and resolves at `Ready{0}`. CBOR frames.
    pub async fn connect(
        url: &url::Url,
        trust: &ServerTrust,
        headers: BTreeMap<String, String>,
    ) -> Result<Self, ClientFailure> {
        let codec = WireCodec::Cbor;
        let session = connect(url, trust, codec.protocol())
            .await
            .map_err(unreachable)?;
        let (mut send, recv) = session
            .open_bi()
            .await
            .ok_or_else(|| unreachable("no control stream"))?;
        let mut control = FrameReader::new(recv, codec, MAX_FRAME_BYTES);
        write(&mut send, &ClientFrame::init(headers)).await?;
        match expect_ready(&mut control, 0).await {
            Ok(()) => Ok(Self {
                session,
                next_id: AtomicU64::new(1),
                _control: Mutex::new((send, control)),
            }),
            Err(failure @ ClientFailure::Refused(_)) => Err(failure),
            // No `{id: 0, err}` frame: the close code is the answer.
            Err(failure) => match session.closed_code().await {
                Some(code) if code != 0 => Err(ClientFailure::Refused(closed_body(code))),
                _ => Err(failure),
            },
        }
    }

    /// Opens `op_id` with `input`, resolving at its `Ready`: nothing
    /// committed after that is missed. A refusal is the `Err`.
    pub async fn subscribe(
        &self,
        op_id: &str,
        input: serde_json::Value,
    ) -> Result<Subscription, ClientFailure> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (mut send, recv) = self
            .session
            .open_bi()
            .await
            .ok_or_else(|| unreachable("the session is gone"))?;
        write(&mut send, &ClientFrame::request(id, op_id, input)).await?;
        send.finish();
        let mut reader = FrameReader::new(recv, WireCodec::Cbor, MAX_FRAME_BYTES);
        expect_ready(&mut reader, id).await?;
        Ok(Subscription { reader, id })
    }
}

/// A live subscription. Dropping it cancels.
pub struct Subscription {
    reader: FrameReader,
    id: u64,
}

impl Subscription {
    /// The next item; `None` once the subscription has ended (cleanly, or
    /// after the `Err` this returned).
    pub async fn next(&mut self) -> Option<Result<serde_json::Value, ClientFailure>> {
        loop {
            return match self.reader.next::<ServerFrame>().await {
                Ok(frame) if frame.id != self.id => continue,
                Ok(frame) => match (frame.next, frame.err) {
                    (Some(item), _) => Some(Ok(item)),
                    (None, Some(err)) => Some(Err(ClientFailure::Refused(err))),
                    (None, None) => None,
                },
                Err(ReadFailure::Ended) => None,
                Err(ReadFailure::Gone) => Some(Err(ClientFailure::Refused(going_away()))),
                Err(ReadFailure::Protocol(error)) => Some(Err(unreachable(error))),
            };
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.reader.stop(0);
    }
}

async fn write(send: &mut SendHalf, frame: &ClientFrame) -> Result<(), ClientFailure> {
    let bytes = WireCodec::Cbor.encode(frame).map_err(unreachable)?;
    send.write_all(&bytes)
        .await
        .map_err(|_| unreachable("the stream closed"))
}

async fn expect_ready(reader: &mut FrameReader, id: u64) -> Result<(), ClientFailure> {
    let frame = tokio::time::timeout(READY_WITHIN, reader.next::<ServerFrame>())
        .await
        .map_err(|_| unreachable("no Ready in time"))?;
    match frame {
        Ok(frame) if frame.id == id && frame.ready.is_some() => Ok(()),
        Ok(ServerFrame { err: Some(err), .. }) => Err(ClientFailure::Refused(err)),
        Ok(_) => Err(unreachable(format!("expected Ready{{{id}}}"))),
        Err(ReadFailure::Gone) if id != 0 => Err(ClientFailure::Refused(going_away())),
        Err(failure) => Err(unreachable(format!("{failure:?}"))),
    }
}
