//! Verifies that two `include_client_schema!` invocations in separate
//! modules don't collide and each expose their own model surface.

pub mod billing {
    use cratestack::include_client_schema;
    include_client_schema!("schemas/billing.cstack");
}

pub mod inventory {
    use cratestack::include_client_schema;
    include_client_schema!("schemas/inventory.cstack");
}

#[test]
fn each_module_owns_its_models() {
    assert!(billing::cratestack_schema::MODELS.contains(&"Invoice"));
    assert!(!billing::cratestack_schema::MODELS.contains(&"Product"));
    assert!(inventory::cratestack_schema::MODELS.contains(&"Product"));
    assert!(!inventory::cratestack_schema::MODELS.contains(&"Invoice"));
}

#[test]
fn generated_types_are_independent() {
    let invoice = billing::cratestack_schema::Invoice {
        id: 1,
        customerId: 42,
        amountCents: 9999,
        paid: false,
    };
    let product = inventory::cratestack_schema::Product {
        id: 1,
        sku: "abc".into(),
        name: "Widget".into(),
        stockOnHand: 12,
    };
    // Round-trip both through serde to prove they're independent types.
    let _: billing::cratestack_schema::Invoice =
        serde_json::from_str(&serde_json::to_string(&invoice).unwrap()).unwrap();
    let _: inventory::cratestack_schema::Product =
        serde_json::from_str(&serde_json::to_string(&product).unwrap()).unwrap();
}
