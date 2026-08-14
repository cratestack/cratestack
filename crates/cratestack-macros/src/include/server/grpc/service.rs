//! The hand-rolled tonic service — no `tonic-build`/`protoc` involved at
//! macro-expansion time (consistent with `message.rs`'s mirror-struct
//! approach: this crate never shells out to `protoc`). The shape below
//! mirrors what `tonic-build` itself emits for a `service { ... }` block
//! (verified directly against `tonic-0.13.1`'s own source —
//! `tonic::server::{Grpc, UnaryService, ServerStreamingService}`,
//! `tonic::codec::ProstCodec`, `tonic::service::Routes` — the same
//! `axum::Router` this workspace already depends on, confirmed aligned by
//! `cargo tree`, closing ticket #171's first acceptance criterion).
//!
//! Each method arm: decode the tonic request's pb message, build exactly
//! the arguments the existing `super::axum::handle_*_dispatch` fn takes
//! (constructing a `CanonicalRequest` whose `path` is the gRPC method path
//! and whose `body` is the pb message re-encoded to bytes — see the
//! "Known gap" note below), call it, and bridge the resulting
//! `axum::Response` back through `cratestack_axum::rpc::bridge_grpc_response`
//! into either the pb response or a `tonic::Status`.
//!
//! **Known gap, flagged rather than hidden:** `docs/design/protobuf.md`
//! §7.3 specifies envelope signing over the *literal unframed wire bytes*.
//! `tonic::server::Grpc::unary`/`server_streaming` decode the pb message
//! via `ProstCodec` before this code ever sees it, so the raw bytes aren't
//! available here — only the already-decoded message. This module
//! re-encodes it via `prost::Message::encode_to_vec()` as the signed
//! `CanonicalRequest.body`, which is byte-identical to the wire only when
//! the client's own encoder produces the same field ordering/varint
//! encoding prost's does (true for every prost-based client; not
//! guaranteed for a hand-rolled or non-Rust encoder that legally produces
//! different-but-equivalent protobuf bytes). Closing this gap for real
//! needs a custom `tonic::codec::Decoder` that captures raw bytes
//! alongside the parsed message — not attempted in this pass.
//!
//! **Procedures (ticket #208).** Every `procedure` declaration gets a
//! method arm too, dispatched through the exact same `super::axum::
//! handle_<name>_dispatch` fn REST/RPC already call (no second dispatch
//! path, same requirement #171 already met for CRUD) — see
//! [`build_procedure_unary_arm`]. A `List`-arity return (`OpKind::
//! Sequence`, mirroring `cratestack-proto::emit::service`'s and
//! `crate::transport::op_descriptors`'s identical `arity == List` check)
//! instead gets [`build_procedure_stream_arm`], using tonic's
//! `ServerStreamingService` — genuinely different wire framing from
//! unary, exercised for the first time in this runtime by this ticket.
//! **What "streaming" means here, stated plainly:** the already-shipped
//! `.proto` contract (ticket #169/#170, `cratestack-proto::emit::
//! service`, protoc-validated in `crates/cratestack-proto/tests/
//! protoc_compiles.rs`) gives a `List`-arity procedure's `<Base>Output`
//! a `repeated result` field — the *whole* list travels in one message,
//! not one item per message. This module honors that existing, tested
//! shape rather than redesigning it: the dispatch call still fully
//! resolves (mirroring how gRPC pins `Content-Type: application/cbor`,
//! never `cbor-seq`, in [`arm_support::request_prelude`] — even a `@stream`-attributed
//! procedure's incremental HTTP path never activates over gRPC, see
//! `crate::axum::procedure::dispatch_tail`'s content-type branch), then
//! the one resulting `<Base>Output` is sent as a single-item
//! `ServerStreamingService` stream (`tokio_stream::once`) rather than a
//! unary response. That is genuine, wire-level gRPC server streaming —
//! `ServerStreamingService`/`Grpc::server_streaming`, unused anywhere in
//! this runtime before this ticket — just not itemwise-incremental
//! production. True incremental item-by-item delivery (matching a
//! `@stream` procedure's own incremental REST/RPC behavior,
//! cratestack#283) would need a custom decoder that consumes the
//! dispatch response's body as it arrives rather than buffering it
//! first; flagged as a smaller, separately-scoped follow-up rather than
//! attempted here.
//!
//! **cratestack#426 note:** the CRUD arm builders this module orchestrates
//! now live in `crud_arms`/`crud_arm_list` (behind the shared
//! `crud_arm_spec::build_unary_arm` helper), and the `ApiServer`
//! tower-service scaffold `build_service` returns lives in `api_server` —
//! both split out to stay under this repo's 200-LoC convention.
//! `arm_support` holds the pieces (the auth/header prelude, the
//! `CratestackError` -> `tonic::Status` mapping) both the CRUD and procedure arm
//! builders share.

use cratestack_core::{Field, Model, Schema, TypeArity};
use quote::quote;

use crate::include::grpc_pb::fields::model_allows_create;

use super::api_server::build_api_server;
use super::crud_arm_list::build_list_arm;
use super::crud_arms::{build_create_arm, build_delete_arm, build_get_arm, build_update_arm};
use super::procedure_arms::{build_procedure_stream_arm, build_procedure_unary_arm};

pub(super) fn build_service(
    schema: &Schema,
    package: &str,
    models_with_pk: &[(&Model, &Field)],
) -> proc_macro2::TokenStream {
    if models_with_pk.is_empty() && schema.procedures.is_empty() {
        return quote! {};
    }
    let service_full_name = format!("{package}.Api");
    let mut arms = Vec::new();
    // `pk` is unused here — each per-verb builder decodes its own PK from
    // the wire message rather than needing this loop's `&Field`; only
    // `models_with_pk()`'s caller (`grpc/mod.rs`, building the `.pb.lock`
    // mirror structs) needs it.
    for (model, _pk) in models_with_pk {
        arms.push(build_list_arm(package, model));
        arms.push(build_get_arm(package, model));
        if model_allows_create(model) {
            arms.push(build_create_arm(package, model));
        }
        arms.push(build_update_arm(package, model));
        arms.push(build_delete_arm(package, model));
    }
    for procedure in &schema.procedures {
        if matches!(procedure.return_type.arity, TypeArity::List) {
            arms.push(build_procedure_stream_arm(package, procedure));
        } else {
            arms.push(build_procedure_unary_arm(package, procedure));
        }
    }

    build_api_server(&service_full_name, &arms)
}
