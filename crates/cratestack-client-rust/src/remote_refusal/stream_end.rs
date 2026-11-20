//! Why the server ended a live subscription (cratestack#1381): the typed
//! reader over the `details.reason` of the terminal `err` frame.

use cratestack_core::{StreamEndReason, Value};

use crate::error::ClientError;

impl ClientError {
    /// The reason the server ended a live subscription, read from the
    /// `details.reason` of a [`ClientError::Remote`] (the terminal `err`
    /// frame, or the `{id:0, err}` frame of a closed socket session).
    ///
    /// `None` for any other variant, an error without `details.reason`, and
    /// a reason this client does not know (a newer server): fall back to the
    /// code and status, as before. Match the result with a wildcard arm,
    /// [`StreamEndReason`] is `#[non_exhaustive]`.
    pub fn stream_end_reason(&self) -> Option<StreamEndReason> {
        let Value::Map(details) = self.remote_error()?.details.as_ref()? else {
            return None;
        };
        match details.get("reason")? {
            Value::String(name) => StreamEndReason::parse(name),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
