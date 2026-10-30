//! [`Credentials`]: what a signer adds to the request the kit sends.

use http::{Extensions, HeaderMap};

/// The credential a signer puts on one request: headers, and request
/// extensions for a credential an in-process layer records (a TLS peer, an
/// envelope's verified signer), which no client can set over the wire.
/// A `HeaderMap` converts into it.
#[derive(Debug, Default, Clone)]
pub struct Credentials {
    /// Headers added to the request.
    pub headers: HeaderMap,
    /// Extensions inserted into the request before the router sees it.
    pub extensions: Extensions,
}

impl Credentials {
    /// Adds an extension, as an in-process layer in front of the router
    /// would.
    #[must_use]
    pub fn with_extension<T: Clone + Send + Sync + 'static>(mut self, value: T) -> Self {
        self.extensions.insert(value);
        self
    }
}

impl From<HeaderMap> for Credentials {
    fn from(headers: HeaderMap) -> Self {
        Self {
            headers,
            extensions: Extensions::new(),
        }
    }
}
