use http::{HeaderMap, HeaderValue};

use super::Credential;

fn headers(values: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for value in values {
        headers.append(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

#[test]
fn bearer_reads_one_token_and_redacts_it() {
    let headers = headers(&["Bearer tok-1"]);
    assert_eq!(
        Credential::bearer(&headers),
        Credential::Present {
            redact: Some("tok-1")
        }
    );
    let lower = self::headers(&["bearer  tok-2 "]);
    assert_eq!(
        Credential::bearer(&lower),
        Credential::Present {
            redact: Some("tok-2")
        }
    );
}

#[test]
fn no_header_or_another_scheme_is_absent() {
    assert!(Credential::bearer(&HeaderMap::new()).is_absent());
    assert!(Credential::bearer(&headers(&["Basic dXNlcjpwYXNz"])).is_absent());
}

#[test]
fn two_headers_or_an_empty_token_are_malformed() {
    assert!(matches!(
        Credential::bearer(&headers(&["Bearer a", "Bearer a"])),
        Credential::Malformed(_)
    ));
    assert!(matches!(
        Credential::bearer(&headers(&["Bearer "])),
        Credential::Malformed(_)
    ));
    assert!(matches!(
        Credential::bearer(&headers(&["Bearer"])),
        Credential::Malformed(_)
    ));
}

#[test]
fn a_named_header_is_present_without_redaction() {
    let mut headers = HeaderMap::new();
    assert!(Credential::header(&headers, "x-signature").is_absent());
    headers.insert("x-signature", HeaderValue::from_static("sig"));
    assert_eq!(
        Credential::header(&headers, "x-signature"),
        Credential::Present { redact: None }
    );
}
