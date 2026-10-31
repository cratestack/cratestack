//! The listener: a second endpoint beside the axum router (design §6a.5),
//! sharing its `AuthProvider` and the schema's subscription dispatcher.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use cratestack_core::{AuthProvider, CratestackError, SubscriptionDispatcher};

use crate::config::{PrincipalKeyFn, WebTransportConfig, WebTransportLimits};
use crate::origin::OriginPolicy;
use crate::principal::SessionRegistry;
use crate::session;
use crate::transport::Listener;

/// State every session of one server reads.
pub(crate) struct Shared<A, D> {
    pub(crate) auth: A,
    pub(crate) dispatcher: D,
    pub(crate) path: String,
    pub(crate) origins: OriginPolicy,
    pub(crate) limits: WebTransportLimits,
    pub(crate) principal_key: PrincipalKeyFn,
    pub(crate) registry: Arc<SessionRegistry>,
}

/// A bound WebTransport listener. Construct with [`Self::bind`], then run
/// [`Self::serve`] (or [`Self::serve_with_shutdown`]) on the runtime.
pub struct WebTransportServer<A, D> {
    listener: Listener,
    shared: Arc<Shared<A, D>>,
}

impl<A: AuthProvider, D: SubscriptionDispatcher> WebTransportServer<A, D> {
    /// Binds the UDP socket. Fails if the certificate does not load or the
    /// address is taken; nothing is accepted until `serve` runs.
    pub fn bind(
        config: WebTransportConfig,
        auth: A,
        dispatcher: D,
    ) -> Result<Self, CratestackError> {
        let listener = Listener::bind(&config)?;
        let shared = Arc::new(Shared {
            auth,
            dispatcher,
            path: config.path,
            origins: config.origins,
            limits: config.limits,
            principal_key: config.principal_key,
            registry: Arc::default(),
        });
        Ok(Self { listener, shared })
    }

    /// The bound address (useful with port 0).
    pub fn local_addr(&self) -> Result<SocketAddr, CratestackError> {
        self.listener.local_addr()
    }

    /// Accepts sessions until the endpoint shuts down.
    pub async fn serve(self) {
        self.serve_with_shutdown(std::future::pending()).await;
    }

    /// Accepts sessions until `shutdown` resolves, then closes the endpoint.
    /// Every open session ends, and clients see what §2.1 promises for any
    /// interruption: their subscriptions end and they resubscribe.
    pub async fn serve_with_shutdown(mut self, shutdown: impl Future<Output = ()>) {
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                incoming = self.listener.accept() => {
                    let Some(incoming) = incoming else { return };
                    tokio::spawn(session::run(Arc::clone(&self.shared), incoming));
                }
                () = &mut shutdown => {
                    self.listener.close("server shutting down");
                    return;
                }
            }
        }
    }
}
