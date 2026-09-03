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
//! # A nested mount needs [`build_rest_op_resolver_with_prefix`]
//!
//! `MatchedPath` reports the full matched path, so under
//! `Router::nest("/api", router)` it reads `/api/$procs/notify` while the
//! generated descriptor says `/$procs/notify`. With the plain constructor
//! every lookup then misses, every op resolves unresolved, and
//! `@no_idempotency` silently does nothing. Pass the mount point to the
//! `_with_prefix` constructor instead. See `super::mount_prefix` for why
//! the prefix is supplied rather than inferred.
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

use super::mount_prefix;

/// Build an op resolver for REST schemas, over the generated
/// `ROUTE_TRANSPORTS` slice, for a router mounted at the root.
///
/// Matches on the route *pattern* (`/widgets/{id}`) rather than the
/// concrete request path (`/widgets/42`), plus the HTTP method — the two
/// together are what identify a REST op, since one path serves up to
/// three verbs.
///
/// Returns [`OpAdmission::unresolved`] when `MatchedPath` is absent (the
/// request hit no route — a 404) or no descriptor matches (a
/// schema/router mismatch). Both still reserve.
///
/// **If the router is nested, use [`build_rest_op_resolver_with_prefix`]**
/// — this constructor compares the matched path exactly, so a nested mount
/// misses every lookup.
pub fn build_rest_op_resolver(
    routes: &'static [RouteTransportDescriptor],
) -> impl Fn(&Request) -> OpAdmission + Send + Sync {
    build_rest_op_resolver_with_prefix("", routes)
}

/// [`build_rest_op_resolver`] for a router mounted under `prefix`, e.g.
/// `build_rest_op_resolver_with_prefix("/api", ROUTE_TRANSPORTS)` to match
/// `Router::nest("/api", router)`.
///
/// `prefix` is forgiving about spelling — `"/api"`, `"/api/"` and `"api"`
/// are the same mount — but strict about boundaries: a path that is not
/// under the prefix *at a segment boundary* resolves unresolved rather
/// than being matched on a truncated remainder. `/apiary/...` is not
/// under `/api`.
pub fn build_rest_op_resolver_with_prefix(
    prefix: &str,
    routes: &'static [RouteTransportDescriptor],
) -> impl Fn(&Request) -> OpAdmission + Send + Sync {
    let prefix = mount_prefix::normalize(prefix);
    move |req: &Request| {
        let Some(matched) = req.extensions().get::<MatchedPath>() else {
            return OpAdmission::unresolved();
        };
        let Some(path) = mount_prefix::strip(matched.as_str(), &prefix) else {
            return OpAdmission::unresolved();
        };
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
