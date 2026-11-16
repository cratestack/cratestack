//! `max_signed_stream_life(..)`: how long a signed subscription may stream
//! (cratestack#1378).

use std::time::Duration;

use super::builder::EnvelopeLayerBuilder;

/// The default longest a signed subscription streams: one hour. Then the
/// stream ends with `unavailable` / `"stream lifetime"` and the client
/// resubscribes with a fresh signed open.
pub const DEFAULT_MAX_SIGNED_STREAM_LIFE: Duration = Duration::from_secs(3600);

impl EnvelopeLayerBuilder {
    /// The longest a signed subscription's stream lives. Default
    /// [`DEFAULT_MAX_SIGNED_STREAM_LIFE`] (one hour); zero is refused by
    /// `build`. The frames of such a stream are not signed, so this bounds how
    /// long one signed open vouches for them. If the credential's `exp`
    /// comes first, it wins; a lifetime too long to represent as a deadline
    /// is not applied, with a warning at open.
    pub fn max_signed_stream_life(mut self, life: Duration) -> Self {
        self.max_signed_stream_life = life;
        self
    }
}
