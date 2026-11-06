//! Step-up end to end (cratestack#1245 story 5, on cratestack#1244's
//! `@authn`, ADR 0022).
//!
//! `payInvoice` declares `@authn(maxAge: 300, amr: ["hwk"])`: the user must
//! have authenticated in the last five minutes with a hardware-bound key.
//! `balance` declares nothing. A user signed in twenty minutes ago with a
//! password reads the balance, and is refused the payment with RFC 9470's
//! step-up signal: a 401 (REST) or `unauthenticated` (RPC, unary and per
//! batch frame) whose `error` is `insufficient_user_authentication`. The
//! generated Rust client reads it as `StepUpRequired`; the app re-proves
//! and retries once through `retry_after_step_up`.
//!
//! - [`issuer`]: the id tokens, which carry `auth_time` and `amr`.
//! - [`device`]: the client's signing key, sent as `Authorization: Signature`.
//! - [`server`]: the generated REST and RPC routers behind
//!   `SignedRequestAuthProvider`, CrateStack's in-house provider.
//! - [`clients`]: the generated REST and RPC clients.
//! - [`flows`]: call, re-prove, retry once, on each binding.

pub mod clients;
pub mod device;
pub mod flows;
pub mod issuer;
pub mod server;
