//! The two request headers a seal binds, read the way the server will.

use cratestack_core::BoundHeaders;
use reqwest::header::HeaderMap;

use crate::error::{ClientError, HeaderPair};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const IF_MATCH: &str = "if-match";

/// The caller's own header list must name each bound header at most once:
/// the header map keeps one value per name, so a second would be dropped
/// silently rather than sealed.
pub(super) fn refuse_duplicates(headers: &[HeaderPair<'_>]) -> Result<(), ClientError> {
    for name in [IDEMPOTENCY_KEY, IF_MATCH] {
        let count = headers
            .iter()
            .filter(|(given, _)| given.eq_ignore_ascii_case(name))
            .count();
        if count > 1 {
            return Err(ClientError::BadInput(format!(
                "header '{name}' is given more than once and cannot be sealed"
            )));
        }
    }
    Ok(())
}

/// `Idempotency-Key` and `If-Match` exactly as the request will carry them.
pub(super) fn bound_headers(headers: &HeaderMap) -> Result<BoundHeaders<'static>, ClientError> {
    let one = |name: &str| -> Result<Option<std::borrow::Cow<'static, str>>, ClientError> {
        let mut values = headers.get_all(name).iter();
        let Some(value) = values.next() else {
            return Ok(None);
        };
        // A second value is one a proxy could fold or drop without the
        // server's rebuilt binding noticing which was meant.
        if values.next().is_some() {
            return Err(ClientError::BadInput(format!(
                "header '{name}' is given more than once and cannot be sealed"
            )));
        }
        let text = value.to_str().map_err(|_| {
            ClientError::BadInput(format!(
                "header '{name}' must be visible ASCII to be sealed"
            ))
        })?;
        // Whitespace at either end is trimmed by hops and parsers, so what
        // was sealed would differ from what arrives.
        if text != text.trim() {
            return Err(ClientError::BadInput(format!(
                "header '{name}' has leading or trailing whitespace and cannot be sealed"
            )));
        }
        Ok(Some(std::borrow::Cow::Owned(text.to_owned())))
    };
    Ok(BoundHeaders {
        idempotency_key: one(IDEMPOTENCY_KEY)?,
        if_match: one(IF_MATCH)?,
    })
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    fn map(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn a_single_clean_value_is_bound() {
        let bound = bound_headers(&map(&[("idempotency-key", "k-1"), ("if-match", "\"v1\"")]))
            .expect("bound");
        assert_eq!(bound.idempotency_key.as_deref(), Some("k-1"));
        assert_eq!(bound.if_match.as_deref(), Some("\"v1\""));
    }

    #[test]
    fn a_duplicated_header_is_refused_locally() {
        for name in ["idempotency-key", "if-match"] {
            let error = bound_headers(&map(&[(name, "a"), (name, "b")])).unwrap_err();
            assert!(
                matches!(error, ClientError::BadInput(_)),
                "{name}: {error:?}"
            );
        }
    }

    #[test]
    fn a_header_list_naming_one_twice_is_refused() {
        assert!(refuse_duplicates(&[("Idempotency-Key", "a"), ("If-Match", "b")]).is_ok());
        let error =
            refuse_duplicates(&[("Idempotency-Key", "a"), ("idempotency-key", "b")]).unwrap_err();
        assert!(matches!(error, ClientError::BadInput(_)));
        assert!(refuse_duplicates(&[("If-Match", "a"), ("If-Match", "a")]).is_err());
    }

    #[test]
    fn surrounding_whitespace_is_refused_locally() {
        for value in [" k", "k ", " k "] {
            for name in ["idempotency-key", "if-match"] {
                let error = bound_headers(&map(&[(name, value)])).unwrap_err();
                assert!(
                    matches!(error, ClientError::BadInput(_)),
                    "{name} {value:?}"
                );
            }
        }
    }
}
