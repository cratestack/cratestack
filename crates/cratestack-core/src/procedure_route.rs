//! Canonical REST route path for a procedure, including its
//! `@api_version` prefix.
//!
//! The server mounts a procedure declared `@api_version("v2")` at
//! `/v2/$procs/<name>` and every other procedure at `/$procs/<name>`, so a
//! schema can serve v1 and v2 of an API side by side. Until this module
//! existed that shape was derived in one place (`cratestack-macros`'s
//! `axum::procedure::route_attrs`) and re-derived, without the version, in
//! five more: the server's own `RouteTransportDescriptor` (which the REST
//! idempotency and rate-limit resolvers match `MatchedPath` against), the
//! generated Rust, TypeScript and Dart clients, and the WireMock stub
//! generator. Every versioned call from a generated client therefore
//! missed with a 404. Same failure shape, and same fix, as cratestack#345's
//! [`crate::route_naming`] for model routes.
//!
//! **Do not reimplement these.** Anything that needs a procedure's REST
//! path calls [`procedure_rest_route_path`]. Callers that embed the path in
//! another language's string literal escape it afterwards (Dart: `$`).
//!
//! `transport rpc` does not use this path. There a procedure's op id is
//! `procedure.<name>` on the server and in every generated client, and
//! `@api_version` does not enter it: procedure names are unique per schema
//! (the parser rejects duplicates), so the op id is already unambiguous.

use crate::schema::{Attribute, Procedure};

/// The `@api_version("...")` value a procedure declares, if any.
///
/// The parser (`validate_procedure_api_version_attribute`) guarantees at
/// most one such attribute, a non-empty quoted string of ASCII
/// alphanumerics, `.`, `-` and `_`, so the value is safe to splice into a
/// URL path segment and into a double- or single-quoted string literal.
pub fn procedure_api_version(procedure: &Procedure) -> Option<&str> {
    api_version_of(&procedure.attributes)
}

/// The HTTP path the server mounts `procedure` at on `transport rest`:
/// `/<version>/$procs/<name>` when it declares `@api_version`, and
/// `/$procs/<name>` otherwise. Relative to the router, so a consumer's
/// mount prefix (`Router::nest`, a client base path) goes in front.
pub fn procedure_rest_route_path(procedure: &Procedure) -> String {
    rest_route_path(&procedure.name, procedure_api_version(procedure))
}

fn api_version_of(attributes: &[Attribute]) -> Option<&str> {
    attributes.iter().find_map(|attribute| {
        attribute
            .raw
            .strip_prefix("@api_version(\"")
            .and_then(|rest| rest.strip_suffix("\")"))
    })
}

fn rest_route_path(name: &str, version: Option<&str>) -> String {
    match version {
        Some(version) => format!("/{version}/$procs/{name}"),
        None => format!("/$procs/{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SourceSpan;

    fn attributes(raw: &[&str]) -> Vec<Attribute> {
        raw.iter()
            .map(|raw| Attribute {
                raw: (*raw).to_owned(),
                span: SourceSpan {
                    start: 0,
                    end: 0,
                    line: 1,
                },
            })
            .collect()
    }

    #[test]
    fn unversioned_procedure_is_mounted_under_dollar_procs() {
        let attrs = attributes(&["@allow(auth() != null)"]);
        assert_eq!(api_version_of(&attrs), None);
        assert_eq!(
            rest_route_path("createPayment", None),
            "/$procs/createPayment"
        );
    }

    #[test]
    fn versioned_procedure_is_prefixed_with_its_version() {
        let attrs = attributes(&["@allow(auth() != null)", "@api_version(\"v2\")"]);
        assert_eq!(api_version_of(&attrs), Some("v2"));
        assert_eq!(
            rest_route_path("createPayment", api_version_of(&attrs)),
            "/v2/$procs/createPayment"
        );
    }

    #[test]
    fn version_is_spliced_verbatim() {
        let attrs = attributes(&["@api_version(\"2024-01.1_beta\")"]);
        assert_eq!(
            rest_route_path("ping", api_version_of(&attrs)),
            "/2024-01.1_beta/$procs/ping"
        );
    }
}
