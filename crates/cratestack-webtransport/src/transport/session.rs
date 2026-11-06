//! The CONNECT request and the accepted session.

use std::net::SocketAddr;
use std::time::Duration;

use cratestack_core::CratestackError;
use web_transport_quinn as wtq;

use super::internal;
use super::stream::{RecvHalf, SendHalf};

/// How long a refused CONNECT keeps its connection for the response to land.
const REJECT_GRACE: Duration = Duration::from_secs(2);
/// Upper bound on waiting for a session close to go out (the crate itself
/// force-closes after a few round trips).
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// A CONNECT request, not yet answered.
pub(crate) struct Incoming {
    request: wtq::Request,
}

impl Incoming {
    pub(super) fn new(request: wtq::Request) -> Self {
        Self { request }
    }

    pub(crate) fn path(&self) -> &str {
        self.request.url.path()
    }

    pub(crate) fn query(&self) -> Option<&str> {
        self.request.url.query()
    }

    /// `host[:port]` the client addressed.
    pub(crate) fn authority(&self) -> String {
        let url = &self.request.url;
        match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            _ => String::new(),
        }
    }

    pub(crate) fn headers(&self) -> &http::HeaderMap {
        &self.request.headers
    }

    pub(crate) fn protocols(&self) -> &[String] {
        &self.request.protocols
    }

    pub(crate) fn remote(&self) -> SocketAddr {
        self.request.conn().remote_address()
    }

    pub(crate) async fn accept(self, protocol: Option<&str>) -> Result<Session, CratestackError> {
        let mut response = wtq::proto::ConnectResponse::OK;
        if let Some(protocol) = protocol {
            response = response.with_protocol(protocol);
        }
        let inner = self.request.respond(response).await.map_err(internal)?;
        Ok(Session {
            inner,
            #[cfg(feature = "client")]
            _client: None,
        })
    }

    /// Answers with `status`, then holds the connection until the client
    /// closes it (or a short grace period passes): dropping it at once
    /// races the connection close against the response, and the client
    /// sees a truncated exchange instead of the status.
    pub(crate) async fn reject(self, status: http::StatusCode) {
        let conn = self.request.conn().clone();
        if self.request.reject(status).await.is_ok() {
            let _ = tokio::time::timeout(REJECT_GRACE, conn.closed()).await;
        }
        conn.close(0u32.into(), b"rejected");
    }
}

/// An accepted session, or (feature `client`) one this process opened.
#[derive(Clone)]
pub(crate) struct Session {
    inner: wtq::Session,
    /// A client session keeps its endpoint for as long as it lives.
    #[cfg(feature = "client")]
    _client: Option<wtq::Client>,
}

impl Session {
    #[cfg(feature = "client")]
    pub(super) fn client(inner: wtq::Session, client: wtq::Client) -> Self {
        Self {
            inner,
            _client: Some(client),
        }
    }

    /// Opens a bidirectional stream; `None` once the session is gone.
    #[cfg(feature = "client")]
    pub(crate) async fn open_bi(&self) -> Option<(SendHalf, RecvHalf)> {
        let (send, recv) = self.inner.open_bi().await.ok()?;
        Some((SendHalf::new(send), RecvHalf::new(recv)))
    }

    /// The session's close code, once the server has closed it.
    #[cfg(feature = "client")]
    pub(crate) async fn closed_code(&self) -> Option<u32> {
        match self.inner.closed().await {
            wtq::SessionError::WebTransportError(wtq::WebTransportError::Closed(code, _)) => {
                Some(code)
            }
            _ => None,
        }
    }

    /// The next client-opened bidirectional stream; `None` once the session
    /// is gone.
    pub(crate) async fn accept_bi(&self) -> Option<(SendHalf, RecvHalf)> {
        let (send, recv) = self.inner.accept_bi().await.ok()?;
        Some((SendHalf::new(send), RecvHalf::new(recv)))
    }

    /// Closes the session with `code`/`reason` and waits until the close
    /// has gone out. The close capsule is written by a task the crate
    /// spawns; returning (and dropping the session) before it lands is how
    /// a browser ends up seeing "Connection lost" instead of the code.
    pub(crate) async fn close(&self, code: u32, reason: &str) {
        self.inner.close(code, reason.as_bytes());
        let _ = tokio::time::timeout(CLOSE_GRACE, self.inner.closed()).await;
    }
}
