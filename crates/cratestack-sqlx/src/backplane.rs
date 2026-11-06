//! Subscription backplanes (`docs/design/subscriptions-websocket.md` §4):
//! [`InProcessBackplane`] for one replica, [`PgNotifyBackplane`] for many.
//!
//! Both fan out locally the same way ([`LocalHub`](cratestack_core::backplane::LocalHub)),
//! which lives in `cratestack-core` with [`InProcessBackplane`], so a
//! `db = None` service can use it without sqlx.

pub(crate) mod notify_payload;
mod pg_notify;

pub use cratestack_core::backplane::{InProcessBackplane, SUBSCRIPTION_BUFFER_CAPACITY};
pub use pg_notify::PgNotifyBackplane;

#[cfg(test)]
mod tests;
