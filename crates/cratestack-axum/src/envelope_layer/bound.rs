//! The request headers the binding authenticates (`bound_headers`,
//! decision S1 after the cratestack#1006 security review).

use std::borrow::Cow;

use cratestack_core::{BoundHeaders, CratestackError};
use http::HeaderMap;
use http::header::IF_MATCH;

/// The header the idempotency layer reads (`idempotency::parse`).
const IDEMPOTENCY_KEY: &str = "idempotency-key";

/// `Idempotency-Key` and `If-Match` as this request carries them, to bind.
///
/// Read from the same headers the idempotency layer and
/// `parse_if_match_version` read (the first value of each, by name, case
/// insensitively). Normalisation, decided here and documented on
/// `cratestack_core::BoundHeaders`: the value is bound **exactly as
/// received**, as UTF-8, untrimmed; those consumers trim, so two spellings
/// they treat alike bind differently, which can only turn a re-spelled
/// header into a `401`, never let one through. A header sent **twice** is
/// refused (`400`) rather than bound: the consumers read the first value,
/// a proxy might forward either, and which one the client signed is not
/// ours to guess. A value that is not UTF-8 cannot be a CBOR text string
/// and is refused the same way (the consumers refuse non-ASCII anyway).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Bound {
    idempotency_key: Option<String>,
    if_match: Option<String>,
}

impl Bound {
    pub(super) fn read(headers: &HeaderMap) -> Result<Self, CratestackError> {
        Ok(Self {
            idempotency_key: one(headers, IDEMPOTENCY_KEY, "Idempotency-Key")?,
            if_match: one(headers, IF_MATCH.as_str(), "If-Match")?,
        })
    }

    pub(super) fn borrowed(&self) -> BoundHeaders<'_> {
        BoundHeaders {
            idempotency_key: self.idempotency_key.as_deref().map(Cow::Borrowed),
            if_match: self.if_match.as_deref().map(Cow::Borrowed),
        }
    }
}

fn one(headers: &HeaderMap, name: &str, label: &str) -> Result<Option<String>, CratestackError> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(CratestackError::BadRequest(format!(
            "{label} must be sent at most once"
        )));
    }
    std::str::from_utf8(value.as_bytes())
        .map(|value| Some(value.to_owned()))
        .map_err(|_| CratestackError::BadRequest(format!("{label} must be UTF-8")))
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue};

    use super::Bound;

    #[test]
    fn bound_as_sent_absent_as_none_twice_refused() {
        let mut headers = HeaderMap::new();
        assert_eq!(Bound::read(&headers).expect("none"), Bound::default());
        headers.insert("Idempotency-Key", HeaderValue::from_static(" k1 "));
        let bound = Bound::read(&headers).expect("one");
        assert_eq!(bound.idempotency_key.as_deref(), Some(" k1 "), "untrimmed");
        assert_eq!(bound.if_match, None);
        headers.append("idempotency-key", HeaderValue::from_static("k2"));
        assert!(Bound::read(&headers).is_err(), "twice");
        let mut headers = HeaderMap::new();
        headers.insert(
            "if-match",
            HeaderValue::from_bytes(b"\"\xff\"").expect("obs-text"),
        );
        assert!(Bound::read(&headers).is_err(), "not UTF-8");
    }
}
