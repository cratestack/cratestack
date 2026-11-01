//! The close and reset codes of a subscription session (design §6.3, §6.6).

/// Session close codes: the WebTransport session close, the WebSocket close
/// frame.
pub mod close_code {
    /// The control stream broke protocol: first frame not `Init`, a frame
    /// that does not decode, or a frame over the size limit.
    pub const PROTOCOL: u32 = 4400;
    /// Authentication was refused (401), or no `Init` arrived in time.
    /// A refusal is preceded by an `{id: 0, err}` frame; a timeout is not.
    pub const UNAUTHENTICATED: u32 = 4401;
    /// A 403 refusal: the provider forbade the caller, or a
    /// re-authentication named a different principal. The `{id: 0, err}`
    /// frame before the close says which.
    pub const FORBIDDEN: u32 = 4403;
    /// Too many open sessions for this principal.
    pub const TOO_MANY_SESSIONS: u32 = 4429;
    /// The `AuthProvider` could not decide (a 5xx: its key server is down).
    /// Not a verdict on the credentials: retry later, do not sign out.
    pub const SERVER_ERROR: u32 = 4500;
}

/// Stream reset codes (sent on one subscription's stream).
pub mod reset_code {
    /// The session is shutting down; treat the subscription as
    /// `unavailable` and resubscribe.
    pub const GOING_AWAY: u32 = 0x01;
    /// The stream broke protocol: not exactly one `Request`, or an `id`
    /// that does not match it.
    pub const PROTOCOL: u32 = 0x02;
}
