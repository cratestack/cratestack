//! Why a socket binding did not open a subscription, which decides whether
//! `Auto` tries the next binding. Only the native socket bindings build an
//! `Unreachable`; SSE errors are returned as they are.

use crate::error::ClientError;

pub(super) enum Failure {
    /// No socket or session: the route is missing, an upgrade is blocked,
    /// UDP is blocked, the network failed. `Auto` tries the next binding.
    #[cfg_attr(
        not(all(
            any(feature = "ws", feature = "webtransport"),
            not(target_arch = "wasm32")
        )),
        allow(dead_code)
    )]
    Unreachable(ClientError),
    /// The server answered: a refusal, a bad input. Never a fallback.
    #[cfg_attr(
        not(all(
            any(feature = "ws", feature = "webtransport"),
            not(target_arch = "wasm32")
        )),
        allow(dead_code)
    )]
    Refused(ClientError),
}

impl Failure {
    #[cfg_attr(
        not(all(
            any(feature = "ws", feature = "webtransport"),
            not(target_arch = "wasm32")
        )),
        allow(dead_code)
    )]
    pub(super) fn into_error(self) -> ClientError {
        match self {
            Self::Unreachable(error) | Self::Refused(error) => error,
        }
    }
}
