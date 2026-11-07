//! Guard keys that appear after compact encoding was chosen.
use reqwest::header::HeaderMap;

use crate::{ClientError, HeaderPair};

/// A compact plan pins a request body's encoding under the per-call
/// `Idempotency-Key`, which wins over the authorizer's (headers.rs). A key the
/// authorizer adds when the call named none was never pinned, so a retry could
/// re-encode under it. Bodiless calls pin nothing and are not checked.
pub(super) fn check(
    compact_planned: bool,
    has_body: bool,
    prepared: bool,
    explicit: &[HeaderPair<'_>],
    headers: &HeaderMap,
) -> Result<(), ClientError> {
    if compact_planned
        && has_body
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

/// Identity headers must not name multiple values before a compact plan is pinned.
pub(super) fn refuse_duplicates(explicit: &[HeaderPair<'_>]) -> Result<(), ClientError> {
    for name in ["idempotency-key", "if-match"] {
        if explicit
            .iter()
            .filter(|(given, _)| given.eq_ignore_ascii_case(name))
            .count()
            > 1
        {
            return Err(ClientError::BadInput(format!(
                "header '{name}' is given more than once"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;
    use reqwest::header::HeaderMap;

    fn sent(keys: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for key in keys {
            headers.append("idempotency-key", key.parse().unwrap());
        }
        headers
    }

    #[test]
    fn an_authorizer_only_key_on_a_body_is_refused() {
        assert!(
            check(
                true,
                true,
                false,
                &[("Idempotency-Key", "k")],
                &sent(&["k"])
            )
            .is_ok()
        );
        assert!(check(true, true, false, &[], &sent(&["k"])).is_err());
        assert!(check(true, true, false, &[], &sent(&[])).is_ok());
    }

    #[test]
    fn bodiless_prepared_and_plain_calls_are_not_checked() {
        assert!(check(true, false, false, &[], &sent(&["from-authorizer"])).is_ok());
        assert!(check(true, true, true, &[], &sent(&["saved"])).is_ok());
        assert!(check(false, true, false, &[], &sent(&["k"])).is_ok());
    }
}
