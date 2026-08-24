//! DB-less proof of the gating predicate and the emitted shape, at the
//! token level — same style as `crate::computed::wire::tests`. Two
//! things are load-bearing here and easy to regress silently:
//!
//! 1. A model with only *bare* `@computed` fields (no `params:`) must
//!    emit no `<Model>ComputedParams` struct at all, and its `get`/`list`
//!    tokens must keep the exact shape they had before this feature
//!    existed (no `computed_params` parameter).
//! 2. A model with a parameterized field must emit the struct AND route
//!    its field through `super::types::<Params>` — never
//!    `super::wire::<Params>` — regardless of which composer calls
//!    [`crate::client::generate_client_module`] (`include_server_schema!`
//!    passes the schema's real computed-bearing set; `include_client_schema!`
//!    always passes an empty one — see that function's own doc). Params
//!    types are never computed-bearing themselves (parser-enforced), so
//!    this must hold under both.

use std::collections::BTreeSet;

use quote::ToTokens;

use crate::client::generate_client_module;
use cratestack_core::TransportStyle;

fn schema(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("fixture schema should parse")
}

fn render(
    schema: &cratestack_core::Schema,
    transport: TransportStyle,
    bearing: &BTreeSet<String>,
) -> String {
    generate_client_module(&schema.models, &schema.procedures, transport, bearing)
        .expect("client module should generate")
        .to_token_stream()
        .to_string()
}

const BARE_ONLY_SCHEMA: &str = "\
model BarePhoto {\n  id Int @id\n  storageKey String\n  thumbnailUrl String @computed\n}\n";

#[test]
fn bare_only_model_emits_no_computed_params_struct() {
    let schema = schema(BARE_ONLY_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        !rendered.contains("ComputedParams"),
        "a model with only bare @computed fields must not emit any \
         ComputedParams struct — rendered: {rendered}"
    );
    assert!(
        !rendered.contains("computed_params"),
        "a model with only bare @computed fields must not grow a \
         computed_params parameter on get/list — rendered: {rendered}"
    );
}

#[test]
fn bare_only_model_get_and_list_tokens_are_unchanged() {
    let schema = schema(BARE_ONLY_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    // Exact pre-feature shape: `get`/`list` never grew a parameter for a
    // model with no parameterized computed field.
    assert!(
        rendered.contains(
            "pub async fn get (& self , id : & i64 , headers : & [:: cratestack :: client_rust :: HeaderPair < '_ >] ,)"
        ),
        "ungated get must keep its exact pre-feature signature — rendered: {rendered}"
    );
    assert!(
        rendered.contains(
            "pub async fn list (& self , query : & [:: cratestack :: client_rust :: QueryPair < '_ >] , headers : & [:: cratestack :: client_rust :: HeaderPair < '_ >] ,)"
        ),
        "ungated list must keep its exact pre-feature signature — rendered: {rendered}"
    );
}

const PARAMETERIZED_SCHEMA: &str = "\
model Photo {\n  id Int @id\n  storageKey String\n  proxyUrl String @computed(params: ProxyParams?)\n}\n\
type ProxyParams {\n  width Int?\n}\n";

#[test]
fn parameterized_model_emits_the_struct_with_super_types_path() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered.contains("struct PhotoComputedParams"),
        "a model with a parameterized computed field must emit its \
         ComputedParams struct — rendered: {rendered}"
    );
    assert!(
        rendered.contains("super :: types :: ProxyParams"),
        "the params field must resolve to super::types::<Params> — \
         params types are never computed-bearing, so there is no wire \
         module to redirect to — rendered: {rendered}"
    );
    assert!(
        !rendered.contains("super :: wire :: ProxyParams"),
        "params types must never resolve to a wire:: path — rendered: {rendered}"
    );
    assert!(
        rendered.contains("fn to_query_value"),
        "the emitted struct must carry its to_query_value helper — rendered: {rendered}"
    );
}

#[test]
fn parameterized_model_get_and_list_gain_the_typed_parameter() {
    let schema = schema(PARAMETERIZED_SCHEMA);
    let rendered = render(&schema, TransportStyle::Rest, &BTreeSet::new());

    assert!(
        rendered
            .contains("computed_params : :: core :: option :: Option < & PhotoComputedParams >"),
        "gated get/list must take a typed Option<&PhotoComputedParams> \
         parameter — rendered: {rendered}"
    );
}

#[test]
fn params_type_path_is_super_types_under_both_bearing_sets() {
    // `include_client_schema!` always passes an empty bearing set;
    // `include_server_schema!` passes the schema's real computed-bearing
    // names (`Photo` itself, since it declares a computed field). Both
    // must resolve the params field the same way — the struct never
    // consults `bearing` at all, so this pins that invariant at the
    // token level rather than relying on it staying true by accident.
    let schema = schema(PARAMETERIZED_SCHEMA);

    let client_composer_rendering = render(&schema, TransportStyle::Rest, &BTreeSet::new());
    let mut server_bearing = BTreeSet::new();
    server_bearing.insert("Photo".to_owned());
    let server_composer_rendering = render(&schema, TransportStyle::Rest, &server_bearing);

    for rendered in [&client_composer_rendering, &server_composer_rendering] {
        assert!(
            rendered.contains("super :: types :: ProxyParams"),
            "params type path must be super::types::<P> regardless of \
             the bearing set the composer passes — rendered: {rendered}"
        );
    }
}

#[test]
fn rpc_transport_gates_the_same_way() {
    let bare = schema(BARE_ONLY_SCHEMA);
    let bare_rendered = render(&bare, TransportStyle::Rpc, &BTreeSet::new());
    assert!(!bare_rendered.contains("ComputedParams"));

    let parameterized = schema(PARAMETERIZED_SCHEMA);
    let parameterized_rendered = render(&parameterized, TransportStyle::Rpc, &BTreeSet::new());
    assert!(parameterized_rendered.contains("struct PhotoComputedParams"));
    assert!(parameterized_rendered.contains("RpcGetInput"));
    assert!(
        !bare_rendered.contains("RpcGetInput"),
        "an ungated model's RPC get must keep using RpcPkInput, not \
         RpcGetInput — rendered: {bare_rendered}"
    );
}
