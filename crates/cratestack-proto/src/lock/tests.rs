use std::collections::BTreeMap;

use cratestack_core::{
    Attribute, EnumDecl, EnumVariant, Field, Model, Schema, SourceSpan, TransportStyle, TypeArity,
    TypeRef,
};

use super::{EnumLock, MessageLock, PbLock, PbLockError, build_lock};

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 0,
    }
}

fn field(name: &str, attrs: &[&str]) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        ty: TypeRef {
            name: "String".to_owned(),
            name_span: span(),
            arity: TypeArity::Required,
            generic_args: vec![],
        },
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

fn model(name: &str, fields: Vec<Field>) -> Model {
    Model {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        fields,
        attributes: vec![],
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

fn schema_with_model(model: Model) -> Schema {
    Schema {
        models: vec![model],
        ..empty_schema()
    }
}

#[test]
fn fresh_schema_assigns_in_declaration_order() {
    let schema = schema_with_model(model(
        "User",
        vec![
            field("id", &[]),
            field("email", &[]),
            field("createdAt", &[]),
        ],
    ));

    let lock = build_lock(&schema, None, &BTreeMap::new()).expect("build_lock");
    let user = &lock.messages["User"];
    assert_eq!(user.fields["id"], 1);
    assert_eq!(user.fields["email"], 2);
    assert_eq!(user.fields["createdAt"], 3);
    assert!(user.reserved.is_empty());
}

#[test]
fn new_field_added_to_existing_lock_gets_next_number() {
    let schema = schema_with_model(model(
        "User",
        vec![field("id", &[]), field("email", &[]), field("phone", &[])],
    ));
    let mut existing_fields = BTreeMap::new();
    existing_fields.insert("id".to_owned(), 1);
    existing_fields.insert("email".to_owned(), 2);
    let mut existing = PbLock {
        version: 1,
        ..PbLock::default()
    };
    existing.messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: existing_fields,
            reserved: vec![],
        },
    );

    let lock = build_lock(&schema, Some(&existing), &BTreeMap::new()).expect("build_lock");
    let user = &lock.messages["User"];
    assert_eq!(user.fields["id"], 1);
    assert_eq!(user.fields["email"], 2);
    assert_eq!(user.fields["phone"], 3);
}

#[test]
fn deleted_field_moves_to_reserved() {
    let schema = schema_with_model(model("User", vec![field("id", &[])]));
    let mut existing_fields = BTreeMap::new();
    existing_fields.insert("id".to_owned(), 1);
    existing_fields.insert("legacyHandle".to_owned(), 2);
    let mut existing = PbLock::default();
    existing.messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: existing_fields,
            reserved: vec![],
        },
    );

    let lock = build_lock(&schema, Some(&existing), &BTreeMap::new()).expect("build_lock");
    let user = &lock.messages["User"];
    assert_eq!(user.fields.get("legacyHandle"), None);
    assert_eq!(user.reserved, vec![2]);
}

#[test]
fn reserved_number_never_reused_by_a_field_with_the_same_name() {
    let with_legacy = schema_with_model(model(
        "User",
        vec![field("id", &[]), field("legacyHandle", &[])],
    ));
    let lock1 = build_lock(&with_legacy, None, &BTreeMap::new()).expect("build_lock 1");
    assert_eq!(lock1.messages["User"].fields["legacyHandle"], 2);

    let without_legacy = schema_with_model(model("User", vec![field("id", &[])]));
    let lock2 = build_lock(&without_legacy, Some(&lock1), &BTreeMap::new()).expect("build_lock 2");
    assert_eq!(lock2.messages["User"].reserved, vec![2]);

    let legacy_readded = schema_with_model(model(
        "User",
        vec![field("id", &[]), field("legacyHandle", &[])],
    ));
    let lock3 = build_lock(&legacy_readded, Some(&lock2), &BTreeMap::new()).expect("build_lock 3");
    assert_eq!(
        lock3.messages["User"].fields["legacyHandle"], 3,
        "re-added field must get a fresh number, not the reserved one"
    );
    assert_eq!(lock3.messages["User"].reserved, vec![2]);
}

#[test]
fn pb_pin_is_honored() {
    let schema = schema_with_model(model(
        "User",
        vec![field("id", &[]), field("email", &["@pb(9)"])],
    ));

    let lock = build_lock(&schema, None, &BTreeMap::new()).expect("build_lock");
    assert_eq!(lock.messages["User"].fields["email"], 9);
}

#[test]
fn pb_pin_colliding_with_used_number_errors() {
    let mut existing_fields = BTreeMap::new();
    existing_fields.insert("id".to_owned(), 1);
    existing_fields.insert("email".to_owned(), 2);
    let mut existing = PbLock::default();
    existing.messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: existing_fields,
            reserved: vec![],
        },
    );

    let schema = schema_with_model(model(
        "User",
        vec![
            field("id", &[]),
            field("email", &[]),
            field("phone", &["@pb(2)"]),
        ],
    ));

    let error =
        build_lock(&schema, Some(&existing), &BTreeMap::new()).expect_err("collision should error");
    assert!(matches!(
        error,
        PbLockError::PinCollidesWithUsed { number: 2, .. }
    ));
}

#[test]
fn pb_pin_colliding_with_reserved_number_errors() {
    let mut existing_fields = BTreeMap::new();
    existing_fields.insert("id".to_owned(), 1);
    let mut existing = PbLock::default();
    existing.messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: existing_fields,
            reserved: vec![5],
        },
    );

    let schema = schema_with_model(model(
        "User",
        vec![field("id", &[]), field("phone", &["@pb(5)"])],
    ));

    let error =
        build_lock(&schema, Some(&existing), &BTreeMap::new()).expect_err("collision should error");
    assert!(matches!(
        error,
        PbLockError::PinCollidesWithReserved { number: 5, .. }
    ));
}

#[test]
fn reserved_range_is_never_auto_assigned() {
    let mut existing_fields = BTreeMap::new();
    existing_fields.insert("a".to_owned(), 18999);
    let mut existing = PbLock::default();
    existing.messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: existing_fields,
            reserved: vec![],
        },
    );

    let schema = schema_with_model(model("User", vec![field("a", &[]), field("b", &[])]));
    let lock = build_lock(&schema, Some(&existing), &BTreeMap::new()).expect("build_lock");
    let assigned = lock.messages["User"].fields["b"];
    assert_eq!(assigned, 20000, "must skip straight over 19000-19999");
}

#[test]
fn build_lock_is_deterministic() {
    let schema = schema_with_model(model(
        "User",
        vec![
            field("id", &[]),
            field("email", &[]),
            field("createdAt", &[]),
        ],
    ));

    let lock1 = build_lock(&schema, None, &BTreeMap::new()).expect("build 1");
    let lock2 = build_lock(&schema, None, &BTreeMap::new()).expect("build 2");
    assert_eq!(lock1.to_toml(), lock2.to_toml());
}

#[test]
fn enum_unspecified_variant_is_always_present_at_zero() {
    let schema = Schema {
        enums: vec![enum_decl("OrderStatus", &["PENDING", "SHIPPED"])],
        ..empty_schema()
    };

    let lock = build_lock(&schema, None, &BTreeMap::new()).expect("build_lock");
    let order_status = &lock.enums["OrderStatus"];
    assert_eq!(order_status.variants["ORDER_STATUS_UNSPECIFIED"], 0);
    assert_eq!(order_status.variants["PENDING"], 1);
    assert_eq!(order_status.variants["SHIPPED"], 2);
}

#[test]
fn toml_round_trips() {
    let mut messages = BTreeMap::new();
    let mut user_fields = BTreeMap::new();
    user_fields.insert("id".to_owned(), 1);
    user_fields.insert("email".to_owned(), 2);
    messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: user_fields,
            reserved: vec![4],
        },
    );

    let mut enums = BTreeMap::new();
    let mut variants = BTreeMap::new();
    variants.insert("ORDER_STATUS_UNSPECIFIED".to_owned(), 0);
    variants.insert("PENDING".to_owned(), 1);
    enums.insert(
        "OrderStatus".to_owned(),
        EnumLock {
            variants,
            reserved: vec![],
        },
    );

    let lock = PbLock {
        version: 1,
        package: Some("shop_api".to_owned()),
        messages,
        enums,
    };

    let toml = lock.to_toml();
    let parsed = PbLock::from_toml(&toml).expect("from_toml");
    assert_eq!(lock, parsed);
}

#[test]
fn deleted_model_keeps_a_tombstone_with_reserved_numbers() {
    let mut existing_fields = BTreeMap::new();
    existing_fields.insert("id".to_owned(), 1);
    existing_fields.insert("email".to_owned(), 2);
    let mut existing = PbLock::default();
    existing.messages.insert(
        "User".to_owned(),
        MessageLock {
            fields: existing_fields,
            reserved: vec![],
        },
    );

    let schema = empty_schema();
    let lock = build_lock(&schema, Some(&existing), &BTreeMap::new()).expect("build_lock");
    let user = &lock.messages["User"];
    assert!(user.fields.is_empty());
    assert_eq!(user.reserved, vec![1, 2]);
}
