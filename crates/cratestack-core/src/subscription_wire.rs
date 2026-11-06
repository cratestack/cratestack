//! What every subscription session binding shares
//! (`docs/design/subscriptions-websocket.md` §6.3, §6.4, §6.6): the frames,
//! admission (`Init`, authentication, refusal close codes), which browser
//! origins may open a session, and the per-principal session count.
//! WebTransport (`cratestack-webtransport`) and WebSocket
//! (`cratestack-websocket`) only move these frames over their own
//! transport.

pub mod admission;
mod codes;
pub mod frame;
pub mod origin;
pub mod principal;

pub use admission::{Handshake, authenticate_session, init_headers, refusal_close_code};
pub use frame::{ClientFrame, ClientMessage, Empty, InitBody, ServerFrame, close_code, reset_code};
pub use origin::OriginPolicy;
pub use principal::{SessionRegistry, SessionSlot, default_principal_key, session_key};
