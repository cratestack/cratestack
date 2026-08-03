# cratestack-grpc

Server-integration runtime for CrateStack `transport grpc` schemas — the
gRPC sibling of `cratestack-axum`. Holds the primitives macro-generated code
calls into: `CoolError` -> `tonic::Status` mapping, `tonic::metadata::MetadataMap`
<-> `http::HeaderMap` conversion (so the existing header-driven `AuthProvider`
ports unchanged), and unframed-body envelope canonicalization for request
signing.

Part of the [CrateStack](https://cratestack.dev) framework. See
`docs/design/protobuf.md` in the main repository for the full design.

Status: runtime primitives (this crate) plus, behind `cratestack-pg`'s
`grpc` Cargo feature, macro-generated pb mirror structs and a hand-rolled
tonic service covering model CRUD (ticket #171) and `procedure`
declarations, unary or server-streaming per their return type's arity
(ticket #208) — mountable into an `axum::Router` via
`cratestack_schema::grpc::into_router(db, registry, codec,
auth_provider)`.

Consumers enabling the `grpc` feature need `prost` as a direct
`Cargo.toml` dependency too (not just reachable via
`cratestack::grpc::prost`) — `prost_derive`'s `#[derive(Message)]` hardcodes
absolute `::prost::...` paths. See `crates/cratestack-grpc/src/lib.rs`'s
module doc for why.
