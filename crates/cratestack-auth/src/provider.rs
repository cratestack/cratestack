//! [`SignedRequestAuthProvider`] — the [`cratestack_core::AuthProvider`]
//! implementation wiring [`SignedRequestVerifier`] into a cratestack server.

use crate::authenticate::authenticate_cratestack_request;
use crate::context_mapping::principal_to_cratestack_context;
use crate::signed_request::SignedRequestVerifier;

#[derive(Clone)]
pub struct SignedRequestAuthProvider {
    verifier: SignedRequestVerifier,
    transport_caller_mode: TransportCallerMode,
}

impl SignedRequestAuthProvider {
    pub fn new(verifier: SignedRequestVerifier) -> Self {
        Self {
            verifier,
            transport_caller_mode: TransportCallerMode::Never,
        }
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
        authenticate_cratestack_request(self.verifier.clone(), request, move |principal| {
            principal_to_cratestack_context(principal, Some("caller"), allow_transport_caller)
        })
    }
}
