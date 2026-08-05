use thiserror::Error;

/// Errors `generate_package` can return. Deliberately fails loudly on
/// anything v1 doesn't know how to synthesize an example for, rather
/// than emitting a plausible-looking but wrong stub — see
/// `docs/design/wiremock-stubs.md`'s "Open questions" section for what's
/// out of scope today and why.
#[derive(Debug, Error)]
pub enum WireMockGeneratorError {
    /// `schema.transport == TransportStyle::Grpc`. gRPC's wire shape
    /// (protobuf, HTTP/2, `.proto`-typed messages) doesn't map onto a
    /// WireMock JSON/HTTP stub the way `rest`/`rpc` do; a `grpc` schema
    /// needs a different mock target entirely (see the design doc's "Is
    /// WireMock the right target at all" discussion).
    #[error(
        "schema declares `transport grpc`, which cratestack-mock-wiremock does not support — \
         WireMock stubs a JSON/HTTP wire shape, not protobuf-over-HTTP/2; see \
         docs/design/wiremock-stubs.md"
    )]
    UnsupportedTransport,

    /// The procedure's return type resolves to a schema construct v1
    /// doesn't know how to synthesize an example value for. In practice
    /// this is defense-in-depth, not something reachable through
    /// `cratestack-parser`'s validated output today: the only known
    /// case (`FindMany<T>`) is already rejected by schema validation
    /// anywhere outside a procedure *argument* position ("built-in
    /// `FindMany<T>` is currently only supported as a procedure
    /// argument type"), so it can't appear in a return type for a
    /// schema that parsed successfully. Kept as a real error rather
    /// than an `unreachable!()` because this crate's public API takes
    /// `&Schema` directly — a caller using
    /// `cratestack_parser::parse_schema_unvalidated` or hand-building a
    /// `Schema` could still hit it.
    #[error(
        "procedure `{procedure}` returns `{type_name}`, which cratestack-mock-wiremock cannot \
         synthesize an example value for yet ({reason}); see docs/design/wiremock-stubs.md's \
         \"Open questions\" section"
    )]
    UnsupportedReturnType {
        procedure: String,
        type_name: String,
        reason: &'static str,
    },

    /// A type reference in the return type's graph names something not
    /// declared in the schema (not a scalar, not a known model/type/enum,
    /// and not one of the built-in generics `Page`/`FindMany`). Schema
    /// validation (`cratestack-parser`) should have already rejected
    /// this before it reaches this crate; kept as a real error rather
    /// than a panic so a library caller that skips validation (or a
    /// future schema construct this crate hasn't learned about yet)
    /// fails with a clear message instead of a panic.
    #[error(
        "procedure `{procedure}`'s return type references `{type_name}`, which is not a known \
         scalar, model, type, or enum in this schema"
    )]
    UnknownType {
        procedure: String,
        type_name: String,
    },

    /// A `Required`-arity field's type graph cycles back to itself with
    /// no scalar/optional/list step along the way to break the
    /// recursion (e.g. `type A { b: B }` / `type B { a: A }` with both
    /// fields required). There is no finite JSON value that satisfies
    /// both types, so this is a real error, not a synthesis gap.
    #[error(
        "procedure `{procedure}`'s return type has an unbreakable required-field cycle through \
         `{type_name}` — no finite example value exists (an `Optional` or `List` step \
         somewhere in the cycle would let the generator terminate it)"
    )]
    UnbreakableCycle {
        procedure: String,
        type_name: String,
    },

    /// Serializing the assembled mapping (schema-derived example value
    /// plus the request/response envelope) to JSON failed. `serde_json`
    /// only fails on a handful of programmer-error conditions (e.g. a
    /// non-finite float, a non-string map key) — surfaced here rather
    /// than unwrapped so a caller gets a `Result` instead of a panic.
    #[error("failed to serialize generated mapping for procedure `{procedure}`: {source}")]
    Serialize {
        procedure: String,
        #[source]
        source: serde_json::Error,
    },
}
