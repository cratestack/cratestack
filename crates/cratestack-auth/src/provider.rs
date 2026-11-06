//! [`SignedRequestAuthProvider`] — the [`cratestack_core::AuthProvider`]
//! implementation wiring [`SignedRequestVerifier`] into a cratestack server.

use crate::authenticate::authenticate_cratestack_request;
use crate::context_mapping::PrincipalMapper;
use crate::signed_request::SignedRequestVerifier;

#[derive(Clone)]
pub struct SignedRequestAuthProvider {
    verifier: SignedRequestVerifier,
    transport_caller_mode: TransportCallerMode,
    mapper: PrincipalMapper,
}

impl SignedRequestAuthProvider {
    /// A provider with the default mapping, [`PrincipalMapper::product`].
    pub fn new(verifier: SignedRequestVerifier) -> Self {
        Self {
            verifier,
            transport_caller_mode: TransportCallerMode::Never,
            mapper: PrincipalMapper::default(),
        }
    }

    /// How a verified principal becomes the request's context: the product
    /// shape plus the principal contract's names (the default), the
    /// contract names only ([`PrincipalMapper::contract`]), or a mapping of
    /// the application's own.
    pub fn principal_mapper(mut self, mapper: PrincipalMapper) -> Self {
        self.mapper = mapper;
        self
    }

    pub fn allow_transport_callers(mut self, mode: TransportCallerMode) -> Self {
        self.transport_caller_mode = mode;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportCallerMode {
    Never,
    SafeReadOnly,
    AllMethods,
}

impl TransportCallerMode {
    fn allows(self, method: &str) -> bool {
        match self {
            Self::Never => false,
            Self::SafeReadOnly => matches!(method, "GET" | "HEAD" | "OPTIONS"),
            Self::AllMethods => true,
        }
    }
}

impl cratestack_core::AuthProvider for SignedRequestAuthProvider {
    type Error = cratestack_core::CratestackError;

    fn authenticate(
        &self,
        request: &cratestack_core::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<cratestack_core::CratestackContext, Self::Error>> + Send
    {
        let allow_transport_caller = self.transport_caller_mode.allows(request.method);
        let mapper = &self.mapper;
        authenticate_cratestack_request(self.verifier.clone(), request, move |principal| {
            mapper.map(principal, Some("caller"), allow_transport_caller)
        })
    }
}
