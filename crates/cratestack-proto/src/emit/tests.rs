use cratestack_core::{
    Attribute, EnumDecl, EnumVariant, Field, Model, Procedure, ProcedureArg, ProcedureKind, Schema,
    SourceSpan, TransportStyle, TypeArity, TypeDecl, TypeRef,
};

use super::{ProtoEmitError, emit_proto, synthesize_messages};
use crate::build_lock;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 0,
    }
}

fn ty(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: span(),
        arity,
        generic_args: vec![],
        int_args: Vec::new(),
    }
}

fn page_ty(item: &str) -> TypeRef {
    TypeRef {
        name: "Page".to_owned(),
        name_span: span(),
        arity: TypeArity::Required,
        generic_args: vec![ty(item, TypeArity::Required)],
        int_args: Vec::new(),
    }
}

fn field(name: &str, type_name: &str, attrs: &[&str]) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        ty: ty(type_name, TypeArity::Required),
        attributes: attrs
            .iter()
            .map(|raw| Attribute {
                raw: (*raw).to_owned(),
                span: span(),
            })
            .collect(),
        span: span(),
    }
}

fn model(name: &str, fields: Vec<Field>, attrs: &[&str]) -> Model {
    Model {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        fields,
        attributes: attrs
            .iter()
            .map(|raw| Attribute {
                raw: (*raw).to_owned(),
                span: span(),
            })
            .collect(),
        span: span(),
    }
}

fn type_decl(name: &str, fields: Vec<Field>) -> TypeDecl {
    TypeDecl {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        fields,
        span: span(),
    }
}

fn enum_decl(name: &str, variants: &[&str]) -> EnumDecl {
    EnumDecl {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        variants: variants
            .iter()
            .map(|name| EnumVariant {
                docs: vec![],
                name: (*name).to_owned(),
                span: span(),
            })
            .collect(),
        span: span(),
    }
}

fn procedure(name: &str, args: Vec<ProcedureArg>, return_type: TypeRef) -> Procedure {
    Procedure {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        kind: ProcedureKind::Query,
        args,
        return_type,
        attributes: vec![],
        span: span(),
    }
}

fn arg(name: &str, type_name: &str) -> ProcedureArg {
    ProcedureArg {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        ty: ty(type_name, TypeArity::Required),
        span: span(),
    }
}

fn empty_schema() -> Schema {
    Schema {
        datasource: None,
        auth: None,
        config_blocks: vec![],
        mixins: vec![],
        models: vec![],
        types: vec![],
        enums: vec![],
        procedures: vec![],
        views: vec![],
        transport: TransportStyle::default(),
        declared_extensions: Default::default(),
    }
}

/// Runs the full ticket #169 pipeline: synthesize -> build_lock -> emit,
/// with `package` pinned so `emit_proto` doesn't reject a missing package.
fn generate(schema: &Schema) -> Result<String, ProtoEmitError> {
    let extra = synthesize_messages(schema)?;
    let mut lock = build_lock(schema, None, &extra).expect("build_lock");
    lock.package = Some("test_pkg".to_owned());
    emit_proto(schema, &lock, &extra, "schema.cstack")
}

#[test]
fn model_message_includes_relation_fields_but_excludes_server_only() {
    let schema = Schema {
        models: vec![
            model(
                "User",
                vec![
                    field("id", "Int", &["@id"]),
                    field("secret", "String", &["@server_only"]),
                    field("profileId", "Int", &[]),
                    field("profile", "Profile", &[]),
                ],
                &["@@allow(\"all\", true)"],
            ),
            model("Profile", vec![field("id", "Int", &["@id"])], &[]),
        ],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    assert!(proto.contains("message User {"));
    assert!(
        !proto.contains("secret"),
        "server_only field must be excluded"
    );
    assert!(
        proto.contains("optional Profile profile"),
        "relation field must be projected as a message reference:\n{proto}"
    );
}

#[test]
fn create_input_omitted_without_create_policy_update_input_always_present() {
    let schema = Schema {
        models: vec![model(
            "Order",
            vec![field("id", "Int", &["@id"]), field("total", "Int", &[])],
            &[],
        )],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    assert!(!proto.contains("message CreateOrderInput"));
    assert!(proto.contains("message UpdateOrderInput"));
    // The primary key is excluded from the update input.
    let update_block = proto
        .split("message UpdateOrderInput {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(!update_block.contains(" id "));
    assert!(update_block.contains("total"));
}

#[test]
fn create_input_excludes_defaulted_fields_when_create_is_allowed() {
    let schema = Schema {
        models: vec![model(
            "Order",
            vec![
                field("id", "Int", &["@id", "@default(autoincrement())"]),
                field("total", "Int", &[]),
            ],
            &["@@allow(\"create\", true)"],
        )],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    let create_block = proto
        .split("message CreateOrderInput {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(
        !create_block.contains("id"),
        "@default field must be excluded from create input"
    );
    assert!(create_block.contains("total"));
}

#[test]
fn type_decl_excludes_server_only_but_keeps_everything_else() {
    let schema = Schema {
        types: vec![type_decl(
            "Filter",
            vec![
                field("query", "String", &[]),
                field("internalNote", "String", &["@server_only"]),
            ],
        )],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    assert!(proto.contains("message Filter {"));
    assert!(proto.contains("query"));
    assert!(!proto.contains("internalNote"));
}

#[test]
fn enum_emission_uses_lock_assigned_numbers_not_rederived() {
    let schema = Schema {
        enums: vec![enum_decl("OrderStatus", &["PENDING", "SHIPPED"])],
        ..empty_schema()
    };
    let extra = synthesize_messages(&schema).expect("synthesize");
    let mut lock = build_lock(&schema, None, &extra).expect("build_lock");
    // Hand-edit the lock the way a merge-conflict resolution would: bump
    // SHIPPED to a number the auto-assigner would never have picked.
    lock.enums
        .get_mut("OrderStatus")
        .unwrap()
        .variants
        .insert("SHIPPED".to_owned(), 42);
    lock.package = Some("test_pkg".to_owned());

    let proto = emit_proto(&schema, &lock, &extra, "schema.cstack").expect("emit_proto");
    assert!(proto.contains("ORDER_STATUS_UNSPECIFIED = 0;"));
    assert!(proto.contains("PENDING = 1;"));
    assert!(
        proto.contains("SHIPPED = 42;"),
        "must read the hand-edited number from the lock, not recompute it:\n{proto}"
    );
}

#[test]
fn two_procedures_returning_the_same_page_item_share_one_message() {
    let schema = Schema {
        models: vec![model("Order", vec![field("id", "Int", &["@id"])], &[])],
        procedures: vec![
            procedure("listOrders", vec![], page_ty("Order")),
            procedure(
                "searchOrders",
                vec![arg("query", "String")],
                page_ty("Order"),
            ),
        ],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    let occurrences = proto.matches("message PageOfOrder {").count();
    assert_eq!(occurrences, 1, "PageOfOrder must be deduplicated:\n{proto}");
    assert!(proto.contains("message PageInfo {"));
    assert!(proto.contains("repeated Order items"));
    assert!(proto.contains("optional int64 total_count"));
    assert!(proto.contains("optional PageInfo page_info"));
    // has_next_page/has_previous_page are never optional; see synth_page.rs.
    assert!(proto.contains("bool has_next_page"));
    assert!(!proto.contains("optional bool has_next_page"));
}

#[test]
fn procedure_output_always_wraps_result_in_a_single_field() {
    let schema = Schema {
        procedures: vec![procedure(
            "getFeed",
            vec![],
            ty("String", TypeArity::Required),
        )],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    assert!(proto.contains("message GetFeedOutput {"));
    assert!(proto.contains("optional string result"));
    assert!(proto.contains("message GetFeedInput {"));
}

#[test]
fn model_named_like_a_synthesized_create_input_collides() {
    let schema = Schema {
        models: vec![
            model(
                "Foo",
                vec![field("id", "Int", &["@id"])],
                &["@@allow(\"create\", true)"],
            ),
            model("CreateFooInput", vec![field("id", "Int", &["@id"])], &[]),
        ],
        ..empty_schema()
    };

    let error = synthesize_messages(&schema).expect_err("collision must be a hard error");
    assert!(matches!(error, ProtoEmitError::MessageNameCollision { .. }));
}

#[test]
fn datetime_field_triggers_timestamp_import_only_when_used() {
    let without_datetime = Schema {
        models: vec![model("Plain", vec![field("id", "Int", &["@id"])], &[])],
        ..empty_schema()
    };
    let proto = generate(&without_datetime).expect("emit_proto");
    assert!(!proto.contains("google/protobuf/timestamp.proto"));

    let with_datetime = Schema {
        models: vec![model(
            "Session",
            vec![
                field("id", "Int", &["@id"]),
                field("createdAt", "DateTime", &[]),
            ],
            &[],
        )],
        ..empty_schema()
    };
    let proto = generate(&with_datetime).expect("emit_proto");
    assert!(proto.contains("import \"google/protobuf/timestamp.proto\";"));
    assert!(proto.contains("optional google.protobuf.Timestamp createdAt"));
}

#[test]
fn decimal_and_json_map_per_the_type_table() {
    let schema = Schema {
        models: vec![model(
            "Wallet",
            vec![
                field("id", "Int", &["@id"]),
                field("balance", "Decimal", &[]),
                field("meta", "Json", &[]),
            ],
            &[],
        )],
        ..empty_schema()
    };

    let proto = generate(&schema).expect("emit_proto");
    assert!(proto.contains("optional string balance"));
    assert!(proto.contains("optional bytes meta = ") && proto.contains("// json"));
}

#[test]
fn missing_package_is_an_error() {
    let schema = empty_schema();
    let extra = synthesize_messages(&schema).expect("synthesize");
    let lock = build_lock(&schema, None, &extra).expect("build_lock");
    let error =
        emit_proto(&schema, &lock, &extra, "schema.cstack").expect_err("no package should error");
    assert!(matches!(error, ProtoEmitError::MissingPackage));
}

#[test]
fn header_states_shape_only_not_wire_bytes() {
    let schema = empty_schema();
    let proto = generate(&schema).expect("emit_proto");
    assert!(proto.contains("does NOT describe the bytes on the wire"));
    assert!(proto.contains("schema.cstack"));
    assert!(
        !proto.contains("service "),
        "no service block in ticket #169"
    );
}
