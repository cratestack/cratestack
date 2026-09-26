//! The canonical form of a request's query string.
//!
//! Moved here from `cratestack-auth`'s `signed_request::canonical`
//! (cratestack#1006), unchanged, because two signers now need the same
//! bytes: `Authorization: Signature` puts it in its signature base, and the
//! COSE envelope binds it as the AAD's `query` element (ADR 0006 §4). The
//! axum envelope layer must not depend on `cratestack-auth` (L1, with its
//! Redis, reqwest and rustls graph), and a second copy would be one more
//! place for a signer and a verifier to disagree about bytes. `cratestack-
//! auth` re-exports this function under its old path.

use std::collections::BTreeMap;

use url::form_urlencoded;

/// `query` decoded as `application/x-www-form-urlencoded`, its pairs
/// ordered by key (values of a repeated key keep their order), and
/// re-encoded. `None`, and a query with no pairs, give `""`.
///
/// Two clients that spell the same query differently (`b=2&a=1` and
/// `a=1&b=2`, `%7E` and `~`) get the same string, so a signature over it
/// survives a proxy or an HTTP library that reorders or re-escapes.
pub fn canonical_query(query: Option<&str>) -> String {
    let Some(query) = query else {
        return String::new();
    };

    let mut grouped = BTreeMap::<String, Vec<String>>::new();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        grouped
            .entry(key.into_owned())
            .or_default()
            .push(value.into_owned());
    }

    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, values) in grouped {
        if values.is_empty() {
            serializer.append_pair(&key, "");
            continue;
        }

        for value in values {
            serializer.append_pair(&key, &value);
        }
    }

    serializer.finish()
}

#[cfg(test)]
mod tests {
    use super::canonical_query;

    #[test]
    fn keys_are_sorted_and_repeated_values_keep_their_order() {
        assert_eq!(canonical_query(Some("z=9&a=2&a=1&b=3")), "a=2&a=1&b=3&z=9");
    }

    #[test]
    fn absent_and_empty_queries_are_the_empty_string() {
        assert_eq!(canonical_query(None), "");
        assert_eq!(canonical_query(Some("")), "");
    }

    #[test]
    fn spelling_differences_canonicalise_to_one_string() {
        assert_eq!(
            canonical_query(Some("b=%7E&a=x+y")),
            canonical_query(Some("a=x%20y&b=~"))
        );
    }
}
