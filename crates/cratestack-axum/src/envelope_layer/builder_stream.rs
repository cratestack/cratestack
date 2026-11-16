//! `max_signed_stream_life(..)`: how long a signed subscription may stream
//! (cratestack#1378).

use std::time::Duration;

use super::builder::EnvelopeLayerBuilder;

/// The default longest a signed subscription streams: one hour. Then the
/// stream ends with `unavailable` / `"stream lifetime"` and the client
/// resubscribes with a fresh signed open.
pub const DEFAULT_MAX_SIGNED_STREAM_LIFE: Duration = Duration::from_secs(3600);

/// The longest `max_signed_stream_life` `build` accepts: 24 hours. A signed
/// open vouches only for its request, so the lifetime is the bound on how long
/// plain frames ride on it; a longer one is refused rather than trusted.
pub const MAX_SIGNED_STREAM_LIFE: Duration = Duration::from_secs(24 * 3600);

impl EnvelopeLayerBuilder {
    /// The longest a signed subscription's stream lives. Default
    /// [`DEFAULT_MAX_SIGNED_STREAM_LIFE`] (one hour); zero, or more than
    /// [`MAX_SIGNED_STREAM_LIFE`] (24 hours), is refused by `build`. The frames of such a stream are not signed, so this bounds how
    /// long one signed open vouches for them. If the credential's `exp`
    /// comes first, it wins.
    pub fn max_signed_stream_life(mut self, life: Duration) -> Self {
        self.max_signed_stream_life = life;
        self
    }
}
