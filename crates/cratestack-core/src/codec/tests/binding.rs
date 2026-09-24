//! `Binding` and `PathParams`: covariance, value equality, and
//! `into_owned` keeping every field (ADR 0006 §4, as amended).

use std::borrow::Cow;

use super::request_binding;
use crate::codec::{Binding, PathParams};

/// Compile-time guard: `Binding` must stay covariant over `'a`. The trait
/// takes `&'a Binding<'a>`, so an invariant field (e.g. a
/// `Cow<'a, [Cow<'a, str>]>`, whose `ToOwned` projection is invariant)
/// would stop generic callers from passing a shorter-lived `&Binding<'_>`.
#[allow(dead_code)]
fn binding_is_covariant<'long: 'short, 'short>(bind: Binding<'long>) -> Binding<'short> {
    bind
}

#[test]
fn path_params_compare_by_value_and_order() {
    let owned = PathParams::Owned(vec!["1".to_owned(), "2".to_owned()]);
    assert_eq!(PathParams::Borrowed(&["1", "2"]), owned);
    assert_ne!(PathParams::Borrowed(&["2", "1"]), owned);
    assert_ne!(PathParams::Borrowed(&["1"]), owned);
    assert_eq!(PathParams::default(), PathParams::EMPTY);
    // `Default` must stay the borrowed empty form: value equality alone
    // would not notice an allocating `Owned(Vec::with_capacity(..))`.
    assert!(matches!(PathParams::default(), PathParams::Borrowed([])));
    assert!(PathParams::EMPTY.is_empty());
    // Bound to the resource: `/accounts/1` and `/accounts/2` share a route
    // template but not a binding.
    let one = Binding {
        method: Cow::Borrowed("GET"),
        route: Cow::Borrowed("/accounts/{id}"),
        path_params: PathParams::Borrowed(&["1"]),
        ..request_binding()
    };
    let two = Binding {
        path_params: PathParams::Borrowed(&["2"]),
        ..one.clone()
    };
    assert_ne!(one, two);
}

#[test]
fn into_owned_keeps_every_field_and_detaches_the_borrow() {
    let route = String::from("/accounts/{id}/payments/{payment}");
    let (account, payment) = (String::from("acc 1"), String::from("pay/2"));
    let params = [account.as_str(), payment.as_str()];
    let query = String::from("a=1&b=2");
    let audience = String::from("payments");
    let borrowed = Binding {
        audience: Cow::Borrowed(&audience),
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed(&route),
        path_params: PathParams::Borrowed(&params),
        query: Some(Cow::Borrowed(&query)),
        schema_sha: [9; 32],
        payload_media_type: Cow::Borrowed("application/cbor"),
        request_digest: Some([3; 32]),
        status: Some(201),
    };
    let owned: Binding<'static> = borrowed.into_owned();
    // Outliving the strings it borrowed from is the point of `into_owned`.
    drop((route, account, payment, query, audience));
    assert!(matches!(owned.audience, Cow::Owned(_)));
    assert_eq!(owned.audience, "payments");
    assert!(matches!(owned.route, Cow::Owned(_)));
    assert_eq!(owned.route, "/accounts/{id}/payments/{payment}");
    assert!(matches!(owned.path_params, PathParams::Owned(_)));
    assert_eq!(
        owned.path_params.iter().collect::<Vec<_>>(),
        ["acc 1", "pay/2"],
        "values and template order survive"
    );
    assert!(matches!(owned.query, Some(Cow::Owned(_))));
    assert_eq!(owned.method, "POST");
    assert_eq!(owned.query.as_deref(), Some("a=1&b=2"));
    assert_eq!(owned.schema_sha, [9; 32]);
    assert_eq!(owned.payload_media_type, "application/cbor");
    assert_eq!(owned.request_digest, Some([3; 32]));
    assert_eq!(owned.status, Some(201));
}
