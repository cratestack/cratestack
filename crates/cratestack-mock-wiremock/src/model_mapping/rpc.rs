//! RPC route derivation for a model's five CRUD stubs (static — see
//! `super`'s module doc for why RPC doesn't get the stateful
//! treatment) — mirrors `generate_model_rpc_dispatch_arms`
//! (`crates/cratestack-macros/src/transport/rpc.rs`): five distinct
//! `POST /rpc/model.<ModelName>.<verb>` op-id routes, none of them a
//! path pattern (unlike REST's `get`/`update`/`delete`, the id lives in
//! the request *body*, never the URL, so there is nothing to wildcard —
//! and, not incidentally, nothing `request.path`-shaped to key a
//! per-record state context off of either).

use cratestack_core::ModelVerb;

use super::VerbRoute;

pub(crate) fn rpc_routes(base: &str, model_name: &str) -> [VerbRoute; 5] {
    let op_path = |verb: ModelVerb| format!("{base}/rpc/{}", verb.rpc_op_id(model_name));

    [
        VerbRoute {
            verb: ModelVerb::List.as_str(),
            method: "POST",
            url: op_path(ModelVerb::List),
            status: 200,
        },
        VerbRoute {
            verb: ModelVerb::Get.as_str(),
            method: "POST",
            url: op_path(ModelVerb::Get),
            status: 200,
        },
        VerbRoute {
            verb: ModelVerb::Create.as_str(),
            method: "POST",
            url: op_path(ModelVerb::Create),
            // Same `StatusCode::CREATED` as REST create — RPC dispatch
            // calls the identical `*_dispatch` fn, just with a different
            // `CanonicalRequest` path (see `handlers_crud.rs`).
            status: 201,
        },
        VerbRoute {
            verb: ModelVerb::Update.as_str(),
            method: "POST",
            url: op_path(ModelVerb::Update),
            status: 200,
        },
        VerbRoute {
            verb: ModelVerb::Delete.as_str(),
            method: "POST",
            url: op_path(ModelVerb::Delete),
            status: 200,
        },
    ]
}
