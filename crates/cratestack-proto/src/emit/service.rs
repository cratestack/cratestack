//! Renders the flat `service Api { ... }` block for `transport grpc`
//! schemas — `docs/design/protobuf.md` §4.6: one service per schema, not
//! one per model. Method names are the op id, PascalCased per segment with
//! the dots dropped (`model.User.list` -> `ModelUserList`,
//! `procedure.publishPost` -> `ProcedurePublishPost`; see
//! `crate::casing::op_id_to_method_name`).
//!
//! Three judgment calls this module makes, called out per ticket #170
//! since the design doc didn't specify them:
//!
//! - **`create`'s existence is gated on `Create<Model>Input` actually
//!   having been synthesized** (i.e. `extra_messages.contains_key(...)`),
//!   which in turn is gated on `@@allow("create"/"all", ...)`
//!   (`mirror::model_allows_create`, ticket #169's shipped, tested
//!   behaviour). This deliberately diverges from
//!   `cratestack-macros::transport::{op_descriptors, rpc}`, which register
//!   `model.<M>.create` **unconditionally** for every model with a primary
//!   key — policy is enforced at runtime there (fail-closed inside the
//!   dispatch handler), and there is no runtime here to fail closed
//!   against. Emitting a `Create` method whose request message `protoc`
//!   can't find would be a dangling reference and strictly worse than this
//!   mismatch with the Rust dispatch's registration list.
//! - **Every other CRUD verb (`list`/`get`/`update`/`delete`) mirrors
//!   `op_descriptors.rs`/`rpc.rs` exactly**: unconditional for any model
//!   with a primary key, regardless of what other policies the model
//!   declares.
//! - **A model with no primary key gets no service methods at all** — not
//!   just `get`/`update`/`delete`, all five, including `list`/`create`.
//!   `rpc.rs`'s `generate_model_rpc_dispatch_arms` registers all five op
//!   ids for a PK-less model too, but every one of them is an
//!   unconditional dispatch error (no runtime dispatch exists here to
//!   error at); the equivalent under a static `.proto` emitter is simply
//!   not emitting the method. The model still gets its ordinary
//!   `message <Model> { ... }` block — that part of ticket #169's behaviour
//!   is transport-independent and unaffected; only the service surface is.

use std::collections::BTreeMap;

use cratestack_core::{Field, Schema, TypeArity};

use super::mirror::model_primary_key_field;
use crate::casing::{op_id_to_method_name, to_pascal_case};

pub(super) struct ServiceMethod {
    method_name: String,
    request_type: String,
    response_type: String,
    server_streaming: bool,
}

pub(super) fn build_service_methods(
    schema: &Schema,
    extra_messages: &BTreeMap<String, Vec<Field>>,
) -> Vec<ServiceMethod> {
    let mut methods = Vec::new();

    for model in &schema.models {
        if model_primary_key_field(model).is_none() {
            continue;
        }
        let name = model.name.as_str();
        methods.push(crud_method(
            name,
            "list",
            format!("{name}RpcListInput"),
            format!("PageOf{name}"),
        ));
        methods.push(crud_method(
            name,
            "get",
            format!("{name}RpcPkInput"),
            name.to_owned(),
        ));
        if extra_messages.contains_key(&format!("Create{name}Input")) {
            methods.push(crud_method(
                name,
                "create",
                format!("Create{name}Input"),
                name.to_owned(),
            ));
        }
        methods.push(crud_method(
            name,
            "update",
            format!("{name}RpcUpdateInput"),
            name.to_owned(),
        ));
        methods.push(crud_method(
            name,
            "delete",
            format!("{name}RpcPkInput"),
            name.to_owned(),
        ));
    }

    for procedure in &schema.procedures {
        let op_id = format!("procedure.{}", procedure.name);
        let base = to_pascal_case(&procedure.name);
        // Mirrors `op_descriptors.rs`'s exact `OpKind::Sequence` condition:
        // list-arity return, nothing else. `Page<T>` returns (`arity ==
        // Required`, `name == "Page"`) stay `Unary` — a page is one
        // message, not a stream of them.
        let server_streaming = matches!(procedure.return_type.arity, TypeArity::List);
        methods.push(ServiceMethod {
            method_name: op_id_to_method_name(&op_id),
            request_type: format!("{base}Input"),
            response_type: format!("{base}Output"),
            server_streaming,
        });
    }

    methods
}

fn crud_method(
    model: &str,
    verb: &str,
    request_type: String,
    response_type: String,
) -> ServiceMethod {
    ServiceMethod {
        method_name: op_id_to_method_name(&format!("model.{model}.{verb}")),
        request_type,
        response_type,
        server_streaming: false,
    }
}

pub(super) fn render_service(methods: &[ServiceMethod]) -> String {
    let mut text = String::from("service Api {\n");
    for method in methods {
        let response = if method.server_streaming {
            format!("stream {}", method.response_type)
        } else {
            method.response_type.clone()
        };
        text.push_str(&format!(
            "  rpc {}({}) returns ({});\n",
            method.method_name, method.request_type, response
        ));
    }
    text.push_str("}\n");
    text
}
