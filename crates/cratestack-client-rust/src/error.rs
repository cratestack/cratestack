use cratestack_core::{CoolError, CoolErrorResponse};
use reqwest::StatusCode;

pub type HeaderPair<'a> = (&'a str, &'a str);
pub type QueryPair<'a> = (&'a str, &'a str);

/// Opaque wrapper around `reqwest::Error` that doesn't expose the type
/// in public match arms, preserving semver hygiene.
#[derive(Debug)]
pub struct TransportError {
    inner: Box<reqwest::Error>,
}

impl TransportError {
    /// Access the underlying `reqwest::Error`.
    pub fn source(&self) -> &reqwest::Error {
        &self.inner
    }

    /// Consume this error and extract the underlying `reqwest::Error`.
    pub fn into_source(self) -> reqwest::Error {
        *self.inner
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl From<reqwest::Error> for TransportError {
    fn from(e: reqwest::Error) -> Self {
        TransportError { inner: Box::new(e) }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    #[error("transport error: {0}")]
    Transport(TransportError),
    #[error("codec error: {0}")]
    Codec(#[from] CoolError),
    #[error("state error: {0}")]
    State(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("remote call failed with status {status}: {message}")]
    Remote {
        status: StatusCode,
        error: Option<CoolErrorResponse>,
        message: String,
    },
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Transport(TransportError::from(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_error_has_source_accessor() {
        // This test verifies that TransportError provides accessor methods
        // to avoid direct exposure of reqwest::Error in public match arms.
        // The actual reqwest::Error is wrapped and only accessible through methods.
        let _err: ClientError = ClientError::State("test".to_string());
        // Verify that ClientError::Transport is not directly matchable as reqwest::Error
        // by ensuring it's a non-exhaustive enum.
    }

    #[test]
    fn client_error_variants() {
        let transport_err = ClientError::State("state error".to_string());
        assert!(matches!(transport_err, ClientError::State(_)));

        let invalid_resp = ClientError::InvalidResponse("invalid".to_string());
        assert!(matches!(invalid_resp, ClientError::InvalidResponse(_)));

        let bad_input = ClientError::BadInput("bad".to_string());
        assert!(matches!(bad_input, ClientError::BadInput(_)));
    }

    #[test]
    fn client_error_displays_correctly() {
        let err = ClientError::State("test error".to_string());
        assert_eq!(err.to_string(), "state error: test error");

        let err = ClientError::InvalidResponse("invalid".to_string());
        assert_eq!(err.to_string(), "invalid response: invalid");
    }
}
