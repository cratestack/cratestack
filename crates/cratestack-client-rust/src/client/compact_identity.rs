//! Guard keys that appear after compact encoding was chosen.
use reqwest::header::HeaderMap;

use crate::{ClientError, HeaderPair};

pub(super) fn check(
    compact_planned: bool,
    prepared: bool,
    explicit: &[HeaderPair<'_>],
    headers: &HeaderMap,
) -> Result<(), ClientError> {
    if compact_planned
        && !prepared
        && headers.contains_key("idempotency-key")
        && !explicit
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("idempotency-key"))
    {
        return Err(ClientError::BadInput(
            "compact retries require Idempotency-Key in per-call headers, not only the authorizer"
                .to_owned(),
        ));
    }
    Ok(())
}
