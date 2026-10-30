//! The chain's own `credential`: what a chain of members answers.

use http::{Extensions, HeaderMap, HeaderValue};

use super::{Verdict, fake, headers};
use crate::chain::first_success;
use crate::context::{AuthProvider, CratestackContext, RequestContext};
use crate::credential::Credential;
use crate::error::CratestackError;

#[test]
fn the_chain_credential_combines_its_members() {
    let extensions = Extensions::new();
    let ask = |names: &[&'static str], bearer: Option<&'static str>| {
        let mut headers = headers(names);
        if let Some(value) = bearer {
            headers.insert(http::header::AUTHORIZATION, HeaderValue::from_static(value));
        }
        let request = RequestContext {
            method: "POST",
            path: "/",
            query: None,
            headers: &headers,
            body: b"",
            extensions: &extensions,
        };
        let chain = first_success(fake("x-a", Verdict::Anonymous)).or(Bearer);
        format!("{:?}", chain.credential(&request))
    };
    assert_eq!(ask(&[], None), "Absent");
    assert_eq!(ask(&["x-a"], None), "Present { redact: None }");
    assert_eq!(
        ask(&[], Some("Bearer tok")),
        "Present { redact: Some(\"tok\") }"
    );
    assert!(ask(&[], Some("Bearer ")).starts_with("Malformed"));
    let unknown = first_success(fake("x-a", Verdict::Anonymous)).or(CachedNothing);
    let headers = HeaderMap::new();
    let request = RequestContext {
        method: "POST",
        path: "/",
        query: None,
        headers: &headers,
        body: b"",
        extensions: &extensions,
    };
    assert_eq!(unknown.credential(&request), Credential::Unknown);
}

/// A provider whose credential is a bearer token.
#[derive(Clone)]
struct Bearer;

impl AuthProvider for Bearer {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::anonymous())
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        Credential::bearer(request.headers)
    }
}

/// A provider that keeps the default credential answer.
#[derive(Clone)]
struct CachedNothing;

impl AuthProvider for CachedNothing {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::anonymous())
    }
}
