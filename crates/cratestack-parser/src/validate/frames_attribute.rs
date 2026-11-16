//! Validation for the `@frames` subscription attribute (cratestack#1378):
//! its argument is read with the one reader the macros use
//! (`cratestack_core::parse_frames_argument`), and it appears at most once.
//!
//! `@frames(sealed)` and `@frames(chain)` are reserved for signed frames
//! (#1020) and chained frames (#1008) and are refused until those exist; any
//! other word is refused too, because a mode nothing reads would silently
//! leave the stream without the trust decision the author meant to record.

use cratestack_core::schema::parse_frames_argument;
use cratestack_core::{Attribute, Procedure};

use crate::diagnostics::{SchemaError, span_error};

/// Checks one `@frames(inner)` of a subscription procedure. `first` is
/// `true` until the first `@frames` of the procedure has been seen.
pub(super) fn validate_frames(
    procedure: &Procedure,
    attribute: &Attribute,
    inner: &str,
    first: &mut bool,
) -> Result<(), SchemaError> {
    if !std::mem::replace(first, false) {
        return Err(span_error(
            format!(
                "subscription procedure `{}` declares more than one `@frames` attribute; \
                 write it once",
                procedure.name
            ),
            attribute.span,
        ));
    }
    parse_frames_argument(inner).map(|_| ()).map_err(|reason| {
        span_error(
            format!("subscription procedure `{}`: {reason}", procedure.name),
            attribute.span,
        )
    })
}
