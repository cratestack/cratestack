//! cratestack#328 regression guards: `db = None` axum-module codegen
//! never emits `ModelRouterState`/`model_router` (provably unreachable —
//! `datasource { provider = "none" }` schemas can never declare a
//! `model`), while `db = Postgres` keeps emitting both, unchanged.

use super::super::super::parse::ServerDb;
use super::super::collect::ServerCollected;
use super::build_axum_module;

fn empty_collected() -> ServerCollected {
    ServerCollected {
        transport_style_str: "rest".to_owned(),
        is_rpc: false,
        mixin_names: Vec::new(),
        model_names: Vec::new(),
        type_names: Vec::new(),
        enum_names: Vec::new(),
        procedure_names: Vec::new(),
        view_names: Vec::new(),
        type_structs: Vec::new(),
        enum_types: Vec::new(),
        computed_field_descriptors: Vec::new(),
        computed_field_resolver_methods: Vec::new(),
        compose_helpers: Vec::new(),
        wire_structs: Vec::new(),
        model_structs: Vec::new(),
        pg_from_row_impls: Vec::new(),
        primary_key_accessor_impls: Vec::new(),
        model_descriptors: Vec::new(),
        field_modules: Vec::new(),
        create_input_structs: Vec::new(),
        update_input_structs: Vec::new(),
        upsert_input_impls: Vec::new(),
        find_many_input_structs: Vec::new(),
        model_accessors: Vec::new(),
        bound_model_accessors: Vec::new(),
        view_structs: Vec::new(),
        view_descriptors: Vec::new(),
        view_pg_from_row_impls: Vec::new(),
        view_accessors: Vec::new(),
        // cratestack#867: empty on purpose, and the axum module is where
        // it has to stay empty — a `query` contributes no route, no
        // handler and no transport constant, so this fixture would be
        // testing the wrong thing if it ever had to populate them.
        query_modules: Vec::new(),
        query_from_row_impls: Vec::new(),
        query_accessors: Vec::new(),
        procedure_modules: Vec::new(),
        procedure_registry_methods: Vec::new(),
        procedure_axum_handler_defs: Vec::new(),
        procedure_axum_routes: Vec::new(),
        procedure_transport_constants: Vec::new(),
        model_axum_handler_defs: Vec::new(),
        model_axum_routes: Vec::new(),
        model_transport_constants: Vec::new(),
        op_descriptor_entries: Vec::new(),
        route_transport_entries: Vec::new(),
        rpc_dispatch_arms: Vec::new(),
        rpc_subscribe_dispatch_arms: Vec::new(),
        generated_client_module: proc_macro2::TokenStream::new(),
        generated_event_module: proc_macro2::TokenStream::new(),
    }
}

#[test]
fn postgres_axum_module_keeps_model_router_state_and_fn() {
    let generated = build_axum_module(&empty_collected(), ServerDb::Postgres).to_string();

    assert!(generated.contains("struct ModelRouterState"));
    assert!(generated.contains("fn model_router"));
    assert!(generated.contains("model_router (db . clone ()"));
}

#[test]
fn none_axum_module_never_emits_model_router_state_or_fn() {
    let generated = build_axum_module(&empty_collected(), ServerDb::None).to_string();

    assert!(
        !generated.contains("ModelRouterState"),
        "db = None must never emit `ModelRouterState` — it's provably unreachable \
         (zero models guaranteed by cratestack#327's datasource guard). generated: {generated}"
    );
    assert!(
        !generated.contains("fn model_router"),
        "db = None must never emit `model_router` — generated: {generated}"
    );
}

#[test]
fn none_axum_module_router_fn_aliases_procedure_router_directly() {
    let generated = build_axum_module(&empty_collected(), ServerDb::None).to_string();

    assert!(generated.contains("fn router"));
    assert!(
        generated.contains("procedure_router (db , registry , resolvers , codec , auth_provider)")
    );
    assert!(!generated.contains(". merge ("));
}

#[test]
fn both_variants_keep_the_same_procedure_router_state_field() {
    let postgres = build_axum_module(&empty_collected(), ServerDb::Postgres).to_string();
    let none = build_axum_module(&empty_collected(), ServerDb::None).to_string();

    // `ProcedureRouterState` keeps its `db: super::Cratestack` field in
    // both variants — the type behind `super::Cratestack` is what
    // changes (see `runtime::none`'s module doc), not this struct's
    // shape. This is the story's documented design resolution: a
    // genuinely different `Cratestack` type, not an optional pool field.
    let expected = "pub struct ProcedureRouterState < R , CR , C , Auth > { pub db : super :: Cratestack , pub registry : R , pub resolvers : CR , pub codec : C , pub auth_provider : Auth , }";
    assert!(postgres.contains(expected));
    assert!(none.contains(expected));
}
