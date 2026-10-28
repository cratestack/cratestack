//! The seam (design §6a.5): the only module that names
//! `web_transport_quinn`. WebTransport over HTTP/3 is still an IETF draft,
//! and the crate implementing it tracks the drafts; when it moves, or is
//! swapped (decision O8), this module is what changes.
//!
//! It also owns the QUIC configuration instead of taking the crate's
//! defaults, because three behaviours the design requires live there:
//! 0-RTT refused (§6a.4 — early data is replayable, and the first bytes a
//! client sends are credentials), the per-session stream budget, and
//! keepalive / idle timeout.

use std::net::SocketAddr;
use std::sync::Arc;

use cratestack_core::CratestackError;
use web_transport_quinn as wtq;
use wtq::quinn;
use wtq::quinn::rustls;

pub use wtq::quinn::rustls::pki_types::{CertificateDer, PrivateKeyDer};

mod session;
mod stream;

pub(crate) use session::{Incoming, Session};
pub(crate) use stream::{RecvHalf, SendHalf};

use crate::config::WebTransportConfig;

/// The bound UDP endpoint, accepting WebTransport CONNECT requests.
pub(crate) struct Listener {
    server: wtq::Server,
}

impl Listener {
    pub(crate) fn bind(config: &WebTransportConfig) -> Result<Self, CratestackError> {
        let provider = wtq::crypto::default_provider();
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(internal)?
            .with_no_client_auth()
            .with_single_cert(
                config.certificate_chain.clone(),
                config.private_key.clone_key(),
            )
            .map_err(internal)?;
        tls.alpn_protocols = vec![wtq::ALPN.as_bytes().to_vec()];
        // rustls defaults to 0 already; stated so a future default change
        // cannot quietly turn 0-RTT on for a listener that must refuse it.
        tls.max_early_data_size = 0;

        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(internal)?;
        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
        let limits = &config.limits;
        let mut transport = quinn::TransportConfig::default();
        // A hard ceiling, set above the subscription limit on purpose: at the
        // limit a client must get an explicit `resource_exhausted` on its
        // `Request` (§6.6), not a stream open that silently waits for QUIC
        // credit. Twice the limit, plus the control and CONNECT streams.
        let bidi = u32::try_from(
            limits
                .subscriptions_per_session
                .saturating_mul(2)
                .saturating_add(2),
        )
        .unwrap_or(u32::MAX);
        transport.max_concurrent_bidi_streams(bidi.into());
        transport.keep_alive_interval(Some(limits.keep_alive));
        transport.max_idle_timeout(Some(limits.idle_timeout.try_into().map_err(internal)?));
        server_config.transport_config(Arc::new(transport));

        let endpoint = quinn::Endpoint::server(server_config, config.bind).map_err(|error| {
            CratestackError::Internal(format!("WebTransport bind failed: {error}"))
        })?;
        Ok(Self {
            server: wtq::Server::new(endpoint),
        })
    }

    pub(crate) fn local_addr(&self) -> Result<SocketAddr, CratestackError> {
        self.server.local_addr().map_err(internal)
    }

    pub(crate) async fn accept(&mut self) -> Option<Incoming> {
        self.server.accept().await.map(Incoming::new)
    }

    /// Closes every connection on the endpoint and stops accepting.
    pub(crate) fn close(&self, reason: &str) {
        self.server.close(0u32.into(), reason.as_bytes());
    }
}

pub(super) fn internal(error: impl std::fmt::Display) -> CratestackError {
    CratestackError::Internal(format!("WebTransport: {error}"))
}
