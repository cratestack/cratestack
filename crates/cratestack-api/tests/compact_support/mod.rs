//! Shared by `tests/compact_rpc.rs`, `compact_rest.rs` and `compact_lock.rs`
//! (compact wire keys, server side): the procedures of
//! `fixtures/compact_{rpc,rest}.cstack`, an anonymous auth provider and the
//! request helpers.

#![allow(dead_code)]

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{HeaderMap, Request, StatusCode};
use cratestack::compact::KeyTables;
use cratestack::{ContractSelector, CratestackContext, CratestackError};
use tower::ServiceExt;

#[derive(Clone)]
pub struct Anonymous;

impl cratestack::AuthProvider for Anonymous {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::anonymous()))
    }
}

pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

/// One request through `router`.
pub async fn call(
    router: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Answer {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body)).expect("request"))
        .await
        .expect("infallible");
    let (parts, body) = response.into_parts();
    Answer {
        status: parts.status,
        headers: parts.headers,
        body: to_bytes(body, usize::MAX).await.expect("body").to_vec(),
    }
}

/// The `Cratestack-Contract` value of `op` at `digest`.
pub fn selector(digest: &[u8; 32]) -> String {
    ContractSelector::of(digest).to_header_value()
}

/// The digest `op` is bound to in a generated `OP_CONTRACTS`.
pub fn digest_of(contracts: &[(&str, [u8; 32])], op: &str) -> [u8; 32] {
    contracts
        .iter()
        .find(|(key, _)| *key == op)
        .unwrap_or_else(|| panic!("no contract for {op}"))
        .1
}

pub fn tables(artifact: &[u8]) -> KeyTables {
    KeyTables::from_bytes(artifact).expect("the generated tables parse")
}

/// The CBOR/JSON bodies the tests send, as the named codec encodes them.
pub fn order_json() -> serde_json::Value {
    serde_json::json!({
        "id": 7, "name": "ada", "note": null,
        "address": {"street": "main", "city": "oslo"},
        "tags": ["a", "b"],
    })
}

/// The procedures of both fixtures, implemented on `Procedures` for the
/// generated module `$m`.
#[macro_export]
macro_rules! compact_procedures {
    ($m:ident) => {
        #[derive(Clone, Default)]
        struct Procedures;

        impl $m::cratestack_schema::procedures::ProcedureRegistry for Procedures {
            fn echo_order(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $m::cratestack_schema::procedures::echo_order::Args,
                _authorized: $m::cratestack_schema::procedures::echo_order::Authorized,
            ) -> impl core::future::Future<
                Output = Result<
                    $m::cratestack_schema::procedures::echo_order::Output,
                    CratestackError,
                >,
            > + Send {
                async move { Ok(args.order) }
            }

            fn list_items(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $m::cratestack_schema::procedures::list_items::Args,
                _authorized: $m::cratestack_schema::procedures::list_items::Authorized,
            ) -> impl core::future::Future<
                Output = Result<
                    $m::cratestack_schema::procedures::list_items::Output,
                    CratestackError,
                >,
            > + Send {
                async move {
                    Ok((0..args.limit)
                        .map(|id| $m::cratestack_schema::Item {
                            id,
                            label: format!("item {id}"),
                        })
                        .collect())
                }
            }

            fn page_items(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $m::cratestack_schema::procedures::page_items::Args,
                _authorized: $m::cratestack_schema::procedures::page_items::Authorized,
            ) -> impl core::future::Future<
                Output = Result<
                    $m::cratestack_schema::procedures::page_items::Output,
                    CratestackError,
                >,
            > + Send {
                async move {
                    let items = (0..args.limit)
                        .map(|id| $m::cratestack_schema::Item {
                            id,
                            label: format!("item {id}"),
                        })
                        .collect();
                    Ok(cratestack::Page::new(
                        items,
                        cratestack::PageInfo::default(),
                    ))
                }
            }

            fn ticks(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: $m::cratestack_schema::procedures::ticks::Args,
                _authorized: $m::cratestack_schema::procedures::ticks::Authorized,
            ) -> impl cratestack::futures::Stream<
                Item = Result<$m::cratestack_schema::Item, CratestackError>,
            > + Send {
                cratestack::async_stream::stream! {
                    for id in 0..args.count {
                        yield Ok($m::cratestack_schema::Item { id, label: format!("tick {id}") });
                    }
                }
            }

            fn boom(
                &self,
                _db: &$m::cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                _args: $m::cratestack_schema::procedures::boom::Args,
                _authorized: $m::cratestack_schema::procedures::boom::Authorized,
            ) -> impl core::future::Future<
                Output = Result<$m::cratestack_schema::procedures::boom::Output, CratestackError>,
            > + Send {
                async move { Err(CratestackError::NotFound("no such order".to_owned())) }
            }
        }
    };
}

pub mod idempotency;

mod prepared;
mod prepared_sealed;
