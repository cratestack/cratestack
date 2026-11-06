//! Ordinal enums (`Pending = 1`, `docs/design/enum-ordinals.md`) in the
//! generated Rust client: a body carries the number and decodes a peer's
//! name too, while a filter carries the name, since a query string is
//! names on both transports (O2).

use cratestack::include_client_schema;

include_client_schema!("tests/fixtures/enum_ordinals.cstack");

use cratestack_schema::types::PaymentStatus;

#[test]
fn a_body_carries_the_number_and_decodes_either_form() {
    let payment = cratestack_schema::models::Payment {
        id: 1,
        status: PaymentStatus::Refunded,
    };
    let json = serde_json::to_value(&payment).expect("serialize");
    assert_eq!(json["status"], serde_json::json!(5));

    for wire in [r#"{"id":1,"status":2}"#, r#"{"id":1,"status":"Approved"}"#] {
        let decoded: cratestack_schema::models::Payment = serde_json::from_str(wire).expect(wire);
        assert_eq!(decoded.status, PaymentStatus::Approved, "{wire}");
    }
    assert!(
        serde_json::from_str::<PaymentStatus>("3").is_err(),
        "3 is reserved"
    );
}

#[test]
fn a_filter_carries_the_name() {
    let where_input = cratestack_schema::PaymentWhere {
        status: Some(cratestack::FieldFilterInput {
            eq: Some(PaymentStatus::Approved),
            ..Default::default()
        }),
        ..Default::default()
    };
    let rendered = format!("{:?}", where_input.to_filters());
    assert!(rendered.contains("\"Approved\""), "{rendered}");
}
