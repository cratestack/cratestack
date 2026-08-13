//! Per-model gRPC method descriptors: the wire path each CRUD verb dials,
//! plus which message names its request/response bodies encode/decode
//! against. `docs/design/protobuf.md` §4.6's `/<package>.Api/<MethodName>`
//! shape, `<package>` from the schema's locked `.pb.lock` and
//! `<MethodName>` from `cratestack_proto::op_id_to_method_name` — the same
//! derivation the Rust server's generated tonic service and
//! `cratestack-proto`'s `.proto` `service` block use, so a
//! `grpcurl`-discovered method name and this generated Dart client's
//! method path are never out of sync.

use cratestack_core::Model;
use cratestack_proto::op_id_to_method_name;
use serde::Serialize;

use crate::dart_types::dart_type;
use crate::idents::{pluralize, to_camel_case};
use crate::naming::{model_allows_create, primary_key_field};

/// `list_method`'s response is always `PageOf<Model>` regardless of
/// whether the model declares `@@paged` — `cratestack-proto::emit::
/// synth_page`'s module doc: every `transport grpc` model's `list` verb
/// gets an implicit `Page<Model>` response on the wire, and the
/// macro-generated service (`build_list_arm`) always wraps the final gRPC
/// response in `pb::PageOf<Model>` even for unpaged models (synthesizing
/// default `PageInfo` there). So unlike REST/RPC's `ModelApiView::
/// list_return_type`, there is no unpaged/paged branch here — no `paged`
/// field on this view at all.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GrpcModelView {
    pub(crate) name: String,
    pub(crate) accessor: String,
    pub(crate) api_name: String,
    pub(crate) primary_key_type: String,
    pub(crate) allows_create: bool,
    pub(crate) create_input_name: String,
    pub(crate) update_input_name: String,
    pub(crate) list_method: GrpcMethod,
    pub(crate) get_method: GrpcMethod,
    pub(crate) create_method: Option<GrpcMethod>,
    pub(crate) update_method: GrpcMethod,
    pub(crate) delete_method: GrpcMethod,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GrpcMethod {
    pub(crate) path: String,
    pub(crate) request_message: String,
    pub(crate) response_message: String,
}

fn grpc_method(
    package: &str,
    model_name: &str,
    verb: &str,
    request: String,
    response: String,
) -> GrpcMethod {
    let op_id = format!("model.{model_name}.{verb}");
    let method_name = op_id_to_method_name(&op_id);
    GrpcMethod {
        path: format!("/{package}.Api/{method_name}"),
        request_message: request,
        response_message: response,
    }
}

pub(crate) fn build_grpc_model_view(package: &str, model: &Model) -> GrpcModelView {
    let primary_key = primary_key_field(model).expect("validated schemas always have an id field");
    let pk_input = format!("{}RpcPkInput", model.name);
    let update_wrapper = format!("{}RpcUpdateInput", model.name);
    let list_input = format!("{}RpcListInput", model.name);
    let page_of = format!("PageOf{}", model.name);
    let create_input = format!("Create{}Input", model.name);
    let update_input = format!("Update{}Input", model.name);

    GrpcModelView {
        name: model.name.clone(),
        accessor: pluralize(&to_camel_case(&model.name)),
        api_name: format!("{}Api", model.name),
        primary_key_type: dart_type(&primary_key.ty, false),
        allows_create: model_allows_create(model),
        create_input_name: create_input.clone(),
        update_input_name: update_input.clone(),
        list_method: grpc_method(package, &model.name, "list", list_input, page_of),
        get_method: grpc_method(
            package,
            &model.name,
            "get",
            pk_input.clone(),
            model.name.clone(),
        ),
        create_method: model_allows_create(model).then(|| {
            grpc_method(
                package,
                &model.name,
                "create",
                create_input,
                model.name.clone(),
            )
        }),
        update_method: grpc_method(
            package,
            &model.name,
            "update",
            update_wrapper,
            model.name.clone(),
        ),
        delete_method: grpc_method(package, &model.name, "delete", pk_input, model.name.clone()),
    }
}
