//! RPC route derivation for a model's five CRUD stubs — mirrors
//! `generate_model_rpc_dispatch_arms`
//! (`crates/cratestack-macros/src/transport/rpc.rs`): five distinct
//! `POST /rpc/model.<ModelName>.<verb>` op-id routes, none of them a
//! path pattern (unlike REST's `get`/`update`/`delete`, the id lives in
//! the request *body*, never the URL, so there is nothing to wildcard).

use super::VerbRoute;

pub(crate) fn rpc_routes(base: &str, model_name: &str) -> [VerbRoute; 5] {
    let op_path = |verb: &str| format!("{base}/rpc/model.{model_name}.{verb}");

    [
        VerbRoute {
            verb: "list",
            method: "POST",
            url: op_path("list"),
            is_pattern: false,
            status: 200,
        },
        VerbRoute {
            verb: "get",
            method: "POST",
            url: op_path("get"),
            is_pattern: false,
            status: 200,
        },
        VerbRoute {
            verb: "create",
            method: "POST",
            url: op_path("create"),
            is_pattern: false,
            // Same `StatusCode::CREATED` as REST create — RPC dispatch
            // calls the identical `*_dispatch` fn, just with a different
            // `CanonicalRequest` path (see `handlers_crud.rs`).
            status: 201,
        },
        VerbRoute {
            verb: "update",
            method: "POST",
            url: op_path("update"),
            is_pattern: false,
            status: 200,
        },
        VerbRoute {
            verb: "delete",
            method: "POST",
            url: op_path("delete"),
            is_pattern: false,
            status: 200,
        },
    ]
}
