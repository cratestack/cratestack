//! `POST /rpc/batch` handler tokens — decodes a sequence of
//! `RpcRequest` frames, re-dispatches each through `rpc_dispatch_inner`,
//! and emits a sequence of `RpcResponseFrame`s in the same order.
//! Per-frame errors don't poison the batch; a malformed batch
//! envelope returns 400. See `docs/design/rpc-transport.md` §3.2.
//!
//! The frame count is capped at `BATCH_MAX_ITEMS` (cratestack#413) —
//! matching every other batch surface
//! (`crates/cratestack-sqlx/src/query/batch/validate.rs`,
//! `crates/cratestack-rusqlite/src/batch/support.rs`) — and checked
//! *before* the per-frame dispatch loop below, since each frame re-enters
//! the exact same `authenticate()` + policy + dispatch path unary calls
//! use (`rpc_dispatch_inner`); an unbounded frame count multiplies that
//! full per-request cost by however many frames a single body can hold.

use quote::quote;

pub(super) fn build_batch_block() -> proc_macro2::TokenStream {
    quote! {
        async fn rpc_batch_dispatch<R, C, Auth>(
            ::cratestack::axum::extract::State(state):
                ::cratestack::axum::extract::State<RpcRouterState<R, C, Auth>>,
            headers: ::cratestack::axum::http::HeaderMap,
            body: ::cratestack::axum::body::Bytes,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            if headers.get(::cratestack::axum::http::header::CONTENT_TYPE).is_some()
                && headers.get("idempotency-key").is_some()
            {
                return rpc_dispatch_error(
                    &state,
                    &headers,
                    ::cratestack::CoolError::BadRequest(
                        "Idempotency-Key header is not supported on /rpc/batch; \
                         use the per-frame `idem` field instead".to_owned(),
                    ),
                );
            }

            let frames: Vec<::cratestack::rpc::RpcRequest> =
                match ::cratestack::__private::decode_rpc_body(&state.codec, &headers, &body) {
                    Ok(frames) => frames,
                    Err(error) => return rpc_dispatch_error(&state, &headers, error),
                };

            // Reject an oversized batch before dispatching a single frame —
            // see this file's module doc. Message mirrors
            // `cratestack-sqlx`'s `validate_batch_size` wording so all
            // batch surfaces speak the same shape.
            if frames.len() > ::cratestack::BATCH_MAX_ITEMS {
                let len = frames.len();
                return rpc_dispatch_error(
                    &state,
                    &headers,
                    ::cratestack::CoolError::Validation(format!(
                        "batch size {len} exceeds maximum of {}",
                        ::cratestack::BATCH_MAX_ITEMS,
                    )),
                );
            }

            let mut responses: Vec<::cratestack::rpc::RpcResponseFrame> =
                Vec::with_capacity(frames.len());
            for frame in frames {
                // Re-encode the frame's opaque `input` value back to
                // codec bytes so we can route it through the same
                // dispatcher as unary.
                let input_bytes = match ::cratestack::__private::encode_rpc_value(
                    &state.codec, &headers, &frame.input,
                ).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        responses.push(::cratestack::rpc::RpcResponseFrame::err(frame.id, &error));
                        continue;
                    }
                };

                // Per-frame state clone — we can't `move` the original
                // because the loop owns it.
                let frame_state = state.clone();
                let frame_headers = headers.clone();
                let response = rpc_dispatch_inner(
                    frame_state,
                    frame_headers,
                    &frame.op,
                    ::cratestack::axum::body::Bytes::from(input_bytes),
                ).await;

                let frame_result = ::cratestack::rpc::response_to_frame(
                    frame.id, response, &state.codec, &headers,
                ).await;
                responses.push(frame_result);
            }

            ::cratestack::encode_transport_result_with_status_for::<
                _,
                Vec<::cratestack::rpc::RpcResponseFrame>,
            >(
                &state.codec,
                &headers,
                &::cratestack::rpc::RPC_BINDING_CAPABILITIES,
                ::cratestack::axum::http::StatusCode::OK,
                Ok(responses),
            )
        }
    }
}
