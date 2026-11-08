//! Both services import the same shared files. Each generates its own copy of
//! every shared declaration, and the copies agree: same enum variants, same
//! `Money` wire shape, same mixin.

use shared_types_import_example::billing::cratestack_schema as billing_schema;
use shared_types_import_example::shipping::cratestack_schema as shipping_schema;

#[test]
fn each_service_generates_the_shared_declarations_beside_its_own() {
    assert_eq!(billing_schema::MODELS, ["Invoice"]);
    assert_eq!(billing_schema::ENUMS, ["Currency"]);
    assert_eq!(billing_schema::TYPES, ["Money"]);
    assert_eq!(billing_schema::MIXINS, ["OrderRecord"]);

    assert_eq!(shipping_schema::MODELS, ["Shipment"]);
    // The root's own enum first, then the imported one: load order.
    assert_eq!(shipping_schema::ENUMS, ["ShipmentStatus", "Currency"]);
    assert_eq!(shipping_schema::TYPES, ["Money"]);
    assert_eq!(shipping_schema::MIXINS, ["OrderRecord"]);
}

#[test]
fn the_shared_enum_has_the_same_variants_in_both_services() {
    for name in ["EUR", "USD", "XAF"] {
        let billing: billing_schema::Currency = name.parse().expect("billing knows it");
        let shipping: shipping_schema::Currency = name.parse().expect("shipping knows it");
        assert_eq!(billing.as_str(), shipping.as_str());
        assert_eq!(
            serde_json::to_value(billing).unwrap(),
            serde_json::to_value(shipping).unwrap()
        );
    }
    assert!("GBP".parse::<billing_schema::Currency>().is_err());
    assert!("GBP".parse::<shipping_schema::Currency>().is_err());
}

#[test]
fn a_billing_money_decodes_as_a_shipping_money() {
    let sent = billing_schema::Money {
        amountMinor: 12_500,
        currency: billing_schema::Currency::XAF,
    };
    let wire = serde_json::to_value(&sent).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({ "amountMinor": 12_500, "currency": "XAF" })
    );

    let received: shipping_schema::Money = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(received.amountMinor, 12_500);
    assert_eq!(received.currency, shipping_schema::Currency::XAF);
    assert_eq!(serde_json::to_value(&received).unwrap(), wire);
}

fn paths(files: &[(&'static str, &'static str)]) -> Vec<&'static str> {
    files.iter().map(|(path, _)| *path).collect()
}

fn contents_of(files: &[(&'static str, &'static str)], path: &str) -> &'static str {
    files
        .iter()
        .find(|(listed, _)| *listed == path)
        .unwrap_or_else(|| panic!("{path} is listed"))
        .1
}

/// `SCHEMA_FILES` lists every file each schema was built from, root first,
/// in load order. Each entry is an `include_str!`, so editing a shared file
/// rebuilds both services.
#[test]
fn each_schema_lists_the_shared_files_in_its_own_load_order() {
    // Billing reaches `currency.cstack` only through `common.cstack`.
    assert_eq!(
        paths(billing_schema::SCHEMA_FILES),
        [
            "billing/schema.cstack",
            "shared/common.cstack",
            "shared/currency.cstack",
        ]
    );
    // Shipping imports `currency.cstack` first and again through
    // `common.cstack`. It is loaded once.
    assert_eq!(
        paths(shipping_schema::SCHEMA_FILES),
        [
            "shipping/schema.cstack",
            "shared/currency.cstack",
            "shared/common.cstack",
        ]
    );

    assert_eq!(
        billing_schema::SCHEMA_FILES[0],
        (billing_schema::SCHEMA_PATH, billing_schema::SCHEMA_SOURCE)
    );
    assert_eq!(
        shipping_schema::SCHEMA_FILES[0],
        (shipping_schema::SCHEMA_PATH, shipping_schema::SCHEMA_SOURCE)
    );
    // `SCHEMA_SOURCE` stays the root's own text.
    assert!(!billing_schema::SCHEMA_SOURCE.contains("enum Currency"));

    for shared in ["shared/common.cstack", "shared/currency.cstack"] {
        assert_eq!(
            contents_of(billing_schema::SCHEMA_FILES, shared),
            contents_of(shipping_schema::SCHEMA_FILES, shared),
            "{shared}"
        );
    }
}
