//! Shared fixtures: a server on an ephemeral port with a scripted
//! dispatcher, and a minimal frame-level client over the native
//! `web-transport-quinn` client.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cratestack_core::{
    CratestackContext, CratestackError, SubscriptionDispatcher, SubscriptionOpen,
    SubscriptionStream, Value,
};
use cratestack_webtransport::{
    CBOR_PROTOCOL, CertificateDer, ClientFrame, FrameDecoder, OriginPolicy, PrivateKeyDer,
    ServerFrame, WebTransportConfig, WebTransportLimits, WebTransportServer, WireCodec,
};
use futures_util::stream;
use sha2::{Digest, Sha256};
use web_transport_quinn as wtq;

pub const ALLOWED_ORIGIN: &str = "http://localhost:5173";

/// `Bearer <id>` authenticates as `<id>`, `Bearer bad` is refused, and no
/// header is anonymous — the shape most `AuthProvider`s have.
pub fn auth(headers: &http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    let Some(value) = headers.get(http::header::AUTHORIZATION) else {
        return Ok(CratestackContext::anonymous());
    };
    match value.to_str().unwrap_or_default().strip_prefix("Bearer ") {
        Some("bad") | None => Err(CratestackError::Unauthorized("bad token".into())),
        Some(id) => Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::String(id.into()),
        )])),
    }
}

/// Flipped when a `test.forever` stream is dropped: proof that a client
/// cancel reached the dispatcher.
#[derive(Clone, Default)]
pub struct Dispatcher {
    pub dropped: Arc<AtomicBool>,
}

struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl SubscriptionDispatcher for Dispatcher {
    async fn open(
        &self,
        open: SubscriptionOpen,
    ) -> Result<SubscriptionStream<serde_json::Value>, CratestackError> {
        match open.op_id.as_str() {
            "test.items" => {
                let n = open.input["n"].as_u64().unwrap_or(0);
                Ok(Box::pin(stream::iter(
                    (0..n).map(|i| Ok(serde_json::json!({ "i": i }))),
                )))
            }
            "test.forever" => {
                let flag = DropFlag(Arc::clone(&self.dropped));
                Ok(Box::pin(stream::unfold(
                    (0u64, flag),
                    |(i, flag)| async move {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        Some((Ok(serde_json::json!(i)), (i + 1, flag)))
                    },
                )))
            }
            "test.fail_mid" => Ok(Box::pin(stream::iter([
                Ok(serde_json::json!("first")),
                Err(CratestackError::Unavailable("subscription lagged".into())),
            ]))),
            "test.whoami" => {
                let who = cratestack_webtransport::default_principal_key(&open.ctx);
                Ok(Box::pin(stream::iter([Ok(serde_json::json!(who))])))
            }
            "test.refuse" => Err(CratestackError::Forbidden(
                "not a member of this room".into(),
            )),
            other => Err(CratestackError::NotFound(format!(
                "unknown subscription op `{other}`"
            ))),
        }
    }
}

pub struct Running {
    pub url: String,
    pub hash: Vec<u8>,
    pub cert: CertificateDer<'static>,
    pub dispatcher: Dispatcher,
}

pub fn cert() -> (
    Vec<CertificateDer<'static>>,
    PrivateKeyDer<'static>,
    Vec<u8>,
) {
    let mut params =
        rcgen::CertificateParams::new(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
            .unwrap();
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(9);
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let der = params.self_signed(&key).unwrap().der().to_vec();
    let hash = Sha256::digest(&der).to_vec();
    (
        vec![CertificateDer::from(der)],
        PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        hash,
    )
}

pub fn start(limits: WebTransportLimits) -> Running {
    let (chain, key, hash) = cert();
    let leaf = chain[0].clone();
    let config = WebTransportConfig::new("127.0.0.1:0".parse().unwrap(), chain, key)
        .origins(OriginPolicy::Allow(vec![ALLOWED_ORIGIN.into()]))
        .limits(limits);
    let dispatcher = Dispatcher::default();
    let server = WebTransportServer::bind(config, auth, dispatcher.clone()).unwrap();
    let port = server.local_addr().unwrap().port();
    tokio::spawn(server.serve());
    Running {
        url: format!("https://127.0.0.1:{port}/rpc/wt"),
        hash,
        cert: leaf,
        dispatcher,
    }
}

impl Running {
    pub async fn connect_with(
        &self,
        request: wtq::proto::ConnectRequest,
    ) -> Result<wtq::Session, wtq::ClientError> {
        client(self.cert.clone()).connect(request).await
    }

    pub fn request(&self) -> wtq::proto::ConnectRequest {
        wtq::proto::ConnectRequest::new(self.url.parse::<url::Url>().unwrap())
            .with_protocol(CBOR_PROTOCOL)
    }

    /// A session authenticated by an `Init` carrying `authorization`.
    pub async fn session(&self, token: Option<&str>) -> Client {
        let session = self.connect_with(self.request()).await.unwrap();
        let mut client = Client::new(session, WireCodec::Cbor).await;
        let headers: BTreeMap<_, _> = token
            .map(|t| ("authorization".to_owned(), format!("Bearer {t}")))
            .into_iter()
            .collect();
        let ready = client.init(headers).await;
        assert!(
            ready.ready.is_some() && ready.id == 0,
            "expected Ready{{0}}, got {ready:?}"
        );
        client
    }
}

/// The client side of one session: its control stream plus helpers.
pub struct Client {
    pub session: wtq::Session,
    pub codec: WireCodec,
    control: Stream,
}

pub struct Stream {
    pub send: wtq::SendStream,
    pub recv: wtq::RecvStream,
    decoder: FrameDecoder,
    codec: WireCodec,
    queued: std::collections::VecDeque<Vec<u8>>,
}

impl Stream {
    pub async fn write(&mut self, frame: &ClientFrame) {
        self.send
            .write_all(&self.codec.encode(frame).unwrap())
            .await
            .unwrap();
    }

    /// The next server frame; `None` at the clean end of the stream.
    pub async fn next(&mut self) -> Option<ServerFrame> {
        loop {
            if let Some(bytes) = self.queued.pop_front() {
                return Some(self.codec.decode(&bytes).unwrap());
            }
            let chunk = tokio::time::timeout(
                Duration::from_secs(5),
                self.recv.read_chunk(64 * 1024, true),
            )
            .await
            .expect("frame within 5s")
            .ok()??;
            self.queued.extend(self.decoder.feed(&chunk.bytes).unwrap());
        }
    }
}

impl Client {
    pub async fn new(session: wtq::Session, codec: WireCodec) -> Self {
        let control = open_stream(&session, codec).await;
        Self {
            session,
            codec,
            control,
        }
    }

    pub async fn init(&mut self, headers: BTreeMap<String, String>) -> ServerFrame {
        self.control.write(&ClientFrame::init(headers)).await;
        self.control.next().await.expect("a reply to Init")
    }

    pub async fn send_control(&mut self, frame: &ClientFrame) {
        self.control.write(frame).await;
    }

    /// Opens a subscription stream and sends its one `Request`.
    pub async fn subscribe(&self, id: u64, op: &str, input: serde_json::Value) -> Stream {
        let mut stream = open_stream(&self.session, self.codec).await;
        stream.write(&ClientFrame::request(id, op, input)).await;
        stream.send.finish().unwrap();
        stream
    }

    /// The session's close code, waiting up to 5s for it.
    pub async fn close_code(&self) -> u32 {
        close_code(&self.session).await
    }
}

pub async fn close_code(session: &wtq::Session) -> u32 {
    match tokio::time::timeout(Duration::from_secs(5), session.closed())
        .await
        .expect("closed within 5s")
    {
        wtq::SessionError::WebTransportError(wtq::WebTransportError::Closed(code, _)) => code,
        other => panic!("expected a WebTransport close, got {other:?}"),
    }
}

async fn open_stream(session: &wtq::Session, codec: WireCodec) -> Stream {
    let opened = tokio::time::timeout(Duration::from_secs(5), session.open_bi());
    let (send, recv) = opened.await.expect("stream credit within 5s").unwrap();
    Stream {
        send,
        recv,
        decoder: FrameDecoder::new(codec, 1 << 20),
        codec,
        queued: Default::default(),
    }
}

/// A native client on an IPv4 socket (`web-transport-quinn`'s builder binds
/// `[::]:0`, which fails on hosts without IPv6) that trusts exactly one
/// certificate — what a browser's `serverCertificateHashes` does.
fn client(expected: CertificateDer<'static>) -> wtq::Client {
    use wtq::quinn;
    use wtq::quinn::rustls;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Pinned {
        expected,
        provider: Arc::clone(&provider),
    });
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![wtq::ALPN.as_bytes().to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
    let endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    wtq::Client::new(endpoint, quinn::ClientConfig::new(Arc::new(crypto)))
}

#[derive(Debug)]
struct Pinned {
    expected: CertificateDer<'static>,
    provider: Arc<wtq::quinn::rustls::crypto::CryptoProvider>,
}

impl wtq::quinn::rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &wtq::quinn::rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: wtq::quinn::rustls::pki_types::UnixTime,
    ) -> Result<wtq::quinn::rustls::client::danger::ServerCertVerified, wtq::quinn::rustls::Error>
    {
        if end_entity.as_ref() == self.expected.as_ref() {
            Ok(wtq::quinn::rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(wtq::quinn::rustls::Error::General(
                "unexpected server certificate".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &wtq::quinn::rustls::DigitallySignedStruct,
    ) -> Result<
        wtq::quinn::rustls::client::danger::HandshakeSignatureValid,
        wtq::quinn::rustls::Error,
    > {
        Err(wtq::quinn::rustls::Error::General(
            "TLS 1.2 is not used by QUIC".into(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &wtq::quinn::rustls::DigitallySignedStruct,
    ) -> Result<
        wtq::quinn::rustls::client::danger::HandshakeSignatureValid,
        wtq::quinn::rustls::Error,
    > {
        wtq::quinn::rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<wtq::quinn::rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
