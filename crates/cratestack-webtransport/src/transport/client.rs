//! The client end of the seam (feature `client`): a QUIC endpoint, the TLS
//! trust a native client brings, and the CONNECT request.
//!
//! The endpoint binds the address family of the server it is about to
//! reach: `web-transport-quinn`'s own builder binds `[::]:0`, which fails on
//! hosts without IPv6. 0-RTT stays off, as on the server (§6a.4): the first
//! bytes are credentials.

use std::net::SocketAddr;
use std::sync::Arc;

use cratestack_core::CratestackError;
use web_transport_quinn as wtq;
use wtq::quinn;
use wtq::quinn::rustls;
use wtq::quinn::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use wtq::quinn::rustls::pki_types::{CertificateDer, ServerName, UnixTime};

use super::session::Session;

/// Which server certificates a client accepts.
#[derive(Debug, Clone)]
pub enum ServerTrust {
    /// A certificate chaining to the platform's root store: production.
    NativeRoots,
    /// Exactly these SHA-256 certificate hashes, whatever their issuer: a
    /// self-signed development certificate (`DevCertificate::sha256`), the
    /// way a browser's `serverCertificateHashes` works.
    CertificateHashes(Vec<[u8; 32]>),
}

/// Why a CONNECT did not produce a session: the network, the handshake, the
/// certificate, or the server's answer. All of them mean "no session".
fn connect_error(error: impl std::fmt::Display) -> CratestackError {
    CratestackError::Unavailable(format!("WebTransport: {error}"))
}

/// CONNECTs to `url`, offering `protocol` as the subprotocol.
pub(crate) async fn connect(
    url: &url::Url,
    trust: &ServerTrust,
    protocol: &str,
) -> Result<Session, CratestackError> {
    let host = url
        .host_str()
        .ok_or_else(|| connect_error("the URL has no host"))?;
    let port = url.port().unwrap_or(443);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let remote = tokio::net::lookup_host((host, port))
        .await
        .map_err(connect_error)?
        .next()
        .ok_or_else(|| connect_error(format!("{host} did not resolve")))?;
    let local: SocketAddr = if remote.is_ipv4() {
        "0.0.0.0:0".parse().expect("a socket address")
    } else {
        "[::]:0".parse().expect("a socket address")
    };
    let endpoint = quinn::Endpoint::client(local).map_err(connect_error)?;
    let client = wtq::Client::new(endpoint, client_config(trust)?);
    let request = wtq::proto::ConnectRequest::new(url.clone()).with_protocol(protocol);
    let inner = client.connect(request).await.map_err(connect_error)?;
    Ok(Session::client(inner, client))
}

fn client_config(trust: &ServerTrust) -> Result<quinn::ClientConfig, CratestackError> {
    let provider = wtq::crypto::default_provider();
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(connect_error)?;
    let mut tls = match trust {
        ServerTrust::NativeRoots => {
            let mut roots = rustls::RootCertStore::empty();
            let native = rustls_native_certs::load_native_certs();
            for error in native.errors {
                tracing::warn!(target: "cratestack", %error, "a native root certificate failed to load");
            }
            for cert in native.certs {
                let _ = roots.add(cert);
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        ServerTrust::CertificateHashes(hashes) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned {
                hashes: hashes.clone(),
                provider,
            }))
            .with_no_client_auth(),
    };
    tls.alpn_protocols = vec![wtq::ALPN.as_bytes().to_vec()];
    tls.enable_early_data = false;
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(connect_error)?;
    Ok(quinn::ClientConfig::new(Arc::new(crypto)))
}

/// Accepts a leaf certificate whose SHA-256 is one of `hashes`; the
/// handshake signature is still verified against that certificate.
#[derive(Debug)]
struct Pinned {
    hashes: Vec<[u8; 32]>,
    provider: wtq::crypto::Provider,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let hash = wtq::crypto::sha256(&self.provider, end_entity);
        if self
            .hashes
            .iter()
            .any(|pinned| pinned[..] == *hash.as_ref())
        {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "the server certificate matches no pinned hash".to_owned(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("QUIC never uses TLS 1.2".to_owned()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
