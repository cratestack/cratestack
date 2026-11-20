//! What the listener does with one `NOTIFY` it read.

use std::sync::Arc;

use cratestack_core::BackplaneMessage;
use cratestack_core::backplane::LocalHub;

use super::notify_payload::{Body, decode};
use crate::sqlx;

/// Delivers the notification `raw` to the local subscribers of its channel.
/// One that cannot be read (bad base64 or CBOR, an unknown version, not an
/// envelope at all) is logged and skipped: nothing a peer sends ends the
/// listener.
pub(super) async fn deliver(hub: &LocalHub, pool: &sqlx::PgPool, raw: &str) {
    let notification = match decode(raw) {
        Ok(notification) => notification,
        Err(reason) => {
            tracing::warn!(target: "cratestack", %reason, "unreadable backplane notification ignored");
            return;
        }
    };
    let message = match notification.body {
        Body::Text(text) => BackplaneMessage::Payload(Arc::from(text.as_ref())),
        Body::Bytes(bytes) => BackplaneMessage::Bytes(Arc::from(bytes.as_ref())),
        Body::Outbox(event_id) => {
            match crate::descriptor::outbox_envelope_json(pool, event_id).await {
                Ok(payload) => BackplaneMessage::Payload(Arc::from(payload)),
                Err(error) => {
                    // The event is lost for this replica: say so rather than skip it.
                    tracing::warn!(target: "cratestack", %error, "large model event unreadable; its channel resyncs");
                    hub.resync(&notification.channel);
                    return;
                }
            }
        }
    };
    hub.deliver(&notification.channel, message);
}
