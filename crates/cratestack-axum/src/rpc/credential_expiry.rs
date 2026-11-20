//! A subscription lives no longer than the credential that opened it
//! (`docs/design/subscriptions-websocket.md` §6.3): the context captured at
//! open is used for every event, so once its `exp` passes the stream ends
//! with `unauthenticated` / `"credential expired"` and the client
//! resubscribes with a fresh credential. A context without `exp` never
//! expires here.
//!
//! This is [`until_principal_ends`](super::until_principal_ends) with the
//! clock alone; the generated open functions use that, with revocation too.

use cratestack_core::SubscriptionStream;

use super::principal_bound::{StreamBounds, until_principal_ends};

/// `items`, ended with `unauthenticated` at `expires_at` (seconds since the
/// epoch, the `exp` claim). Already expired: the first item is that error.
pub fn until_credential_expires<T: Send + 'static>(
    items: SubscriptionStream<T>,
    expires_at: Option<i64>,
) -> SubscriptionStream<T> {
    until_principal_ends(
        items,
        StreamBounds {
            expires_at,
            revocation: None,
        },
    )
}

#[cfg(test)]
mod tests;
