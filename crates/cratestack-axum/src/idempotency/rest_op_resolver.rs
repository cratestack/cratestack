//! Op resolver for REST transport: recover the `OpAdmission` facts the
//! schema declared about the route a request matched.
//!
//! Deliberately shaped after `crate::ratelimit::rest_ops_filter`, which
//! solved the identical "a `tower::Layer` sits above routing and has to
//! learn which op it is about to dispatch" problem for cratestack#474.
//! Same mechanism, same `MatchedPath` trick, same
//! `RouteTransportDescriptor::path` `{param}` shape — see that module for
//! the full write-up of why `Router::layer` and `Router::route_layer` both
//! populate `MatchedPath` and how they differ on 404s.
//!
//! # The fail-closed direction is inverted here, and that is the point
//!
//! The rate-limit filter fails closed by *rate-limiting* on a lookup miss.
//! This resolver fails closed by *reserving* on a lookup miss — it returns
//! [`OpAdmission::unresolved`], whose `idempotent_by_default` is `false`.
//! Both are "when in doubt, apply the protection"; they only look opposite
//! because the two flags are polarised differently.
//!
//! One consequence is worth stating plainly, because it is the whole
//! byte-identity argument for ADR 0015 slice 1: an `IdempotencyLayer` with
//! **no** resolver installed treats every request as unresolved, and
//! therefore reserves exactly the set of requests it reserved before this
//! crate existed. Installing a resolver is opt-in, and opting out is
//! bit-for-bit the old behaviour.

use axum::extract::{MatchedPath, Request};
use cratestack_core::RouteTransportDescriptor;
use cratestack_exec::OpAdmission;

/// Build an op resolver for REST schemas, over the generated
/// `ROUTE_TRANSPORTS` slice.
///
/// Matches on the route *pattern* (`/widgets/{id}`) rather than the
/// concrete request path (`/widgets/42`), plus the HTTP method — the two
/// together are what identify a REST op, since one path serves up to
/// three verbs.
///
/// Returns [`OpAdmission::unresolved`] when `MatchedPath` is absent (the
/// request hit no route — a 404) or no descriptor matches (a
/// schema/router mismatch). Both still reserve.
pub fn build_rest_op_resolver(
    routes: &'static [RouteTransportDescriptor],
) -> impl Fn(&Request) -> OpAdmission + Send + Sync {
    move |req: &Request| {
        let Some(matched) = req.extensions().get::<MatchedPath>() else {
            return OpAdmission::unresolved();
        };
        let path = matched.as_str();
        let method = req.method().as_str();

        // Linear search, matching `rest_ops_filter`: the slice is not
        // sorted, and a schema's route count is small enough that
        // building an index would cost more than it saves.
        routes
            .iter()
            .find(|route| route.method == method && route.path == path)
            .map_or_else(OpAdmission::unresolved, OpAdmission::from)
    }
}
