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
//! the same policy + dispatch path unary calls use (`rpc_dispatch_inner`);
//! an unbounded frame count multiplies that full per-request cost by
//! however many frames a single body can hold.
//!
//! ## Authenticate the envelope once, not once per frame
//!
//! `rpc_dispatch_inner`'s per-op dispatch functions each independently
//! call `AuthProvider::authenticate` against a `CanonicalRequest` built
//! from `/rpc/<op_id>` and that op's own (here: re-encoded) frame body —
//! the identity that's actually correct for the real unary route those
//! functions were written for. Re-entering them once per batch frame
//! reused that unary-shaped identity unchanged, but the request a batch
//! client actually signs is the *whole* `POST /rpc/batch` call — one
//! method/path/body/signature covering every queued frame at once, per
//! `docs/design/rpc-transport.md` §5. Handing every frame's dispatch a
//! fabricated `/rpc/<op_id>` + single-frame body to authenticate against
//! is simply the wrong request for any `AuthProvider` whose verdict is
//! bound to the bytes it's given (a body-hash-bound request-signing
//! scheme, for instance) — and, independently, calling `authenticate()`
//! N times for one client-issued request breaks any provider that treats
//! a successful authentication as consuming a single-use nonce.
//!
//! So this handler authenticates the real envelope — `POST`,
//! `RPC_BATCH_PATH`, the untouched raw `body` this handler received —
//! exactly once, and hands every frame's dispatch a
//! [`::cratestack::CachedAuthProvider`] that already holds that one
//! verdict instead of re-deriving (and re-verifying) it per frame. The
//! per-op dispatch functions are untouched: `rpc_dispatch_inner` is
//! generic over its `Auth` parameter independently of the router's own,
//! so this only changes which concrete `AuthProvider` batch dispatch
//! hands them for the lifetime of one HTTP request.

use quote::quote;

pub(super) fn build_batch_block() -> proc_macro2::TokenStream {
    quote! {
        async fn rpc_batch_dispatch<R, CR, C, Auth>(
            ::cratestack::axum::extract::State(state):
                ::cratestack::axum::extract::State<RpcRouterState<R, CR, C, Auth>>,
            headers: ::cratestack::axum::http::HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: ::cratestack::axum::body::Bytes,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            if headers.get(::cratestack::axum::http::header::CONTENT_TYPE).is_some()
                && headers.get("idempotency-key").is_some()
            {
                return rpc_dispatch_error(
                    &state,
                    &headers,
                    ::cratestack::CratestackError::BadRequest(
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
            // batch surfaces speak the same shape. Cheap and
            // signature-independent, so it still runs (and still rejects,
            // with zero `authenticate()` calls) before the envelope is
            // authenticated below.
            if frames.len() > ::cratestack::BATCH_MAX_ITEMS {
                let len = frames.len();
                return rpc_dispatch_error(
                    &state,
                    &headers,
                    ::cratestack::CratestackError::Validation(format!(
                        "batch size {len} exceeds maximum of {}",
                        ::cratestack::BATCH_MAX_ITEMS,
                    )),
                );
            }

            // Authenticate the real envelope exactly once — the actual
            // `POST /rpc/batch` request the client signed, untouched raw
            // body included — before dispatching a single frame. See this
            // file's module doc for why every frame must share this one
            // verdict rather than each re-deriving its own.
            let batch_request = request_context(
                "POST",
                ::cratestack::rpc::RPC_BATCH_PATH,
                None,
                &headers,
                body.as_ref(),
                &client_ip_ctx.extensions,
            );
            let batch_ctx = match state.auth_provider.authenticate(&batch_request).await {
                Ok(ctx) => ctx,
                Err(error) => {
                    let error: ::cratestack::CratestackError = error.into();
                    return rpc_dispatch_error(&state, &headers, error);
                }
            };
            let cached_auth = ::cratestack::CachedAuthProvider::new(batch_ctx);

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
                // because the loop owns it. `auth_provider` is the
                // envelope-level `cached_auth` (see above), NOT
                // `state.auth_provider` — every frame shares the one
                // real verdict instead of each independently
                // re-authenticating a fabricated per-op identity.
                let frame_state = RpcRouterState {
                    db: state.db.clone(),
                    registry: state.registry.clone(),
                    resolvers: state.resolvers.clone(),
                    codec: state.codec.clone(),
                    auth_provider: cached_auth.clone(),
                };
                let frame_headers = headers.clone();
                let frame_client_ip_ctx = client_ip_ctx.clone();
                let response = rpc_dispatch_inner(
                    frame_state,
                    frame_headers,
                    &frame.op,
                    ::cratestack::axum::body::Bytes::from(input_bytes),
                    frame_client_ip_ctx,
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
