use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use http::{Extensions, HeaderMap, HeaderValue};

use super::first_success;
use crate::context::{AuthProvider, CratestackContext, RequestContext};
use crate::credential::Credential;
use crate::error::{AuthRefusal, CratestackError};
use crate::value::Value;

#[derive(Clone, Copy)]
enum Verdict {
    Accept(&'static str),
    Anonymous,
    Refuse,
}

/// A provider whose credential is the header `header`, which answers
/// `verdict` and counts its calls.
#[derive(Clone)]
struct Fake {
    header: &'static str,
    verdict: Verdict,
    calls: Arc<AtomicUsize>,
}

fn fake(header: &'static str, verdict: Verdict) -> Fake {
    Fake {
        header,
        verdict,
        calls: Arc::default(),
    }
}

impl Fake {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AuthProvider for Fake {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.verdict {
            Verdict::Accept(id) => Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::String(id.to_owned()),
            )])),
            Verdict::Anonymous => Ok(CratestackContext::anonymous()),
            Verdict::Refuse => Err(AuthRefusal::unauthenticated("refused")
                .with_member("error", Value::String(self.header.to_owned()))
                .into_error()),
        }
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        Credential::header(request.headers, self.header)
    }
}

fn headers(names: &[&'static str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for name in names {
        headers.insert(*name, HeaderValue::from_static("x"));
    }
    headers
}

async fn run<P: AuthProvider>(
    provider: &P,
    headers: &HeaderMap,
) -> Result<CratestackContext, CratestackError> {
    let extensions = Extensions::new();
    let request = RequestContext {
        method: "POST",
        path: "/rpc/procedure.ping",
        query: None,
        headers,
        body: b"",
        extensions: &extensions,
    };
    provider.authenticate(&request).await.map_err(Into::into)
}

fn id(context: &CratestackContext) -> &str {
    context.principal_actor_id().unwrap_or("")
}

#[tokio::test]
async fn the_first_member_that_authenticates_wins() {
    let (a, b) = (
        fake("x-a", Verdict::Accept("a")),
        fake("x-b", Verdict::Accept("b")),
    );
    let chain = first_success(a.clone()).or(b.clone());
    let context = run(&chain, &headers(&["x-a", "x-b"])).await.unwrap();
    assert_eq!(id(&context), "a");
    assert_eq!((a.calls(), b.calls()), (1, 0));
}

#[tokio::test]
async fn an_absent_credential_skips_the_member_without_calling_it() {
    let (a, b) = (
        fake("x-a", Verdict::Refuse),
        fake("x-b", Verdict::Accept("b")),
    );
    let chain = first_success(a.clone()).or(b.clone());
    let context = run(&chain, &headers(&["x-b"])).await.unwrap();
    assert_eq!(id(&context), "b");
    assert_eq!((a.calls(), b.calls()), (0, 1));
}

#[tokio::test]
async fn a_refusal_is_never_rescued_by_a_later_member() {
    let (a, b) = (
        fake("x-a", Verdict::Refuse),
        fake("x-b", Verdict::Accept("b")),
    );
    let chain = first_success(a.clone()).or(b.clone());
    let error = run(&chain, &headers(&["x-a", "x-b"])).await.unwrap_err();
    let CratestackError::AuthRefused(refusal) = error else {
        panic!("expected the first member's refusal, got {error:?}");
    };
    assert_eq!(refusal.members()["error"], Value::String("x-a".to_owned()));
    assert_eq!((a.calls(), b.calls()), (1, 0));
}

#[tokio::test]
async fn an_anonymous_answer_goes_on_to_the_next_member() {
    let (a, b, c) = (
        fake("x-a", Verdict::Anonymous),
        fake("x-b", Verdict::Anonymous),
        fake("x-c", Verdict::Accept("c")),
    );
    let chain = first_success(a.clone()).or(b.clone()).or(c.clone());
    let context = run(&chain, &headers(&["x-a", "x-b", "x-c"])).await.unwrap();
    assert_eq!(id(&context), "c");
    assert_eq!((a.calls(), b.calls(), c.calls()), (1, 1, 1));
}

#[tokio::test]
async fn no_member_authenticating_is_anonymous() {
    let (a, b) = (
        fake("x-a", Verdict::Anonymous),
        fake("x-b", Verdict::Accept("b")),
    );
    let chain = first_success(a.clone()).or(b.clone());
    let context = run(&chain, &headers(&["x-a"])).await.unwrap();
    assert!(!context.is_authenticated());
    assert_eq!((a.calls(), b.calls()), (1, 0));
}

#[tokio::test]
async fn a_provider_without_a_credential_method_is_always_called() {
    // A header closure keeps the default `Credential::Unknown`.
    let closure = |_: &HeaderMap| -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::String("closure".to_owned()),
        )]))
    };
    let a = fake("x-a", Verdict::Accept("a"));
    let chain = first_success(a.clone()).or(closure);
    let context = run(&chain, &HeaderMap::new()).await.unwrap();
    assert_eq!(id(&context), "closure");
    assert_eq!(a.calls(), 0);
}

#[tokio::test]
async fn the_first_anonymous_context_is_kept() {
    // What a provider put in its anonymous answer (here a client ip)
    // survives the chain, as it did behind a hand-written wrapper.
    let tagged = |_: &HeaderMap| -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::anonymous().with_client_ip("10.0.0.1"))
    };
    let chain = first_success(tagged).or(fake("x-b", Verdict::Anonymous));
    let context = run(&chain, &headers(&["x-b"])).await.unwrap();
    assert!(!context.is_authenticated());
    assert_eq!(context.client_ip(), Some("10.0.0.1"));
}

#[path = "chain_credential_tests.rs"]
mod credential;
