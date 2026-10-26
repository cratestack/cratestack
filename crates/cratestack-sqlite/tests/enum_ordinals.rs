//! Ordinal enums (`Pending = 1`, `docs/design/enum-ordinals.md`) through
//! the embedded ORM: a variant is stored as its number (an SQLite
//! INTEGER, not the name), read back as the variant, filtered by it, and
//! serialized as the number while still decoding a peer's name.

use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/enum_ordinals.cstack");

use cratestack_schema::models::Payment;
use cratestack_schema::{CreatePaymentInput, PAYMENT_MODEL, PaymentStatus, UpdatePaymentInput};

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&PAYMENT_MODEL))
                .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn stored(runtime: &RusqliteRuntime, id: i64) -> (String, i64, Option<i64>) {
    runtime
        .with_connection(|conn| {
            Ok(conn
                .query_row(
                    "SELECT typeof(status), status, previous FROM payments WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .expect("row stored"))
        })
        .unwrap()
}

#[test]
fn a_variant_is_stored_as_its_ordinal_and_read_back() {
    let runtime = setup();
    let delegate = ModelDelegate::<Payment, i64>::new(&runtime, &PAYMENT_MODEL);

    let created = delegate
        .create(CreatePaymentInput {
            id: 1,
            status: PaymentStatus::Approved,
            previous: Some(PaymentStatus::Refunded),
        })
        .run()
        .expect("create");
    assert_eq!(stored(&runtime, 1), ("integer".to_owned(), 2, Some(5)));

    let fetched = delegate.find_unique(1).run().expect("find").expect("row");
    assert_eq!(fetched, created);
    assert_eq!(fetched.status, PaymentStatus::Approved);

    delegate
        .create(CreatePaymentInput {
            id: 2,
            status: PaymentStatus::Pending,
            previous: None,
        })
        .run()
        .expect("create with a NULL optional");
    assert_eq!(stored(&runtime, 2), ("integer".to_owned(), 1, None));

    let updated = delegate
        .update(2)
        .set(UpdatePaymentInput {
            status: Some(PaymentStatus::Refunded),
            previous: Some(Some(PaymentStatus::Pending)),
        })
        .run()
        .expect("update");
    assert_eq!(updated.status, PaymentStatus::Refunded);
    assert_eq!(stored(&runtime, 2), ("integer".to_owned(), 5, Some(1)));

    let refunded = delegate
        .find_many()
        .where_(cratestack_schema::payment::status().eq(PaymentStatus::Refunded))
        .run()
        .expect("filter by variant");
    assert_eq!(refunded.iter().map(|p| p.id).collect::<Vec<_>>(), [2]);
}

#[test]
fn the_wire_form_is_the_number_and_a_name_still_decodes() {
    assert_eq!(PaymentStatus::Refunded.ordinal(), 5);
    assert_eq!(PaymentStatus::try_from(2_i16), Ok(PaymentStatus::Approved));
    assert!(PaymentStatus::try_from(3_i16).is_err(), "3 is reserved");
    // Names stay the type's text form.
    assert_eq!(PaymentStatus::Approved.to_string(), "Approved");

    let json = serde_json::to_string(&PaymentStatus::Approved).unwrap();
    assert_eq!(json, "2");
    for wire in ["2", "\"Approved\""] {
        let decoded: PaymentStatus = serde_json::from_str(wire).unwrap();
        assert_eq!(decoded, PaymentStatus::Approved, "{wire}");
    }
    for bad in ["3", "\"Settled\"", "70000", "-1"] {
        assert!(serde_json::from_str::<PaymentStatus>(bad).is_err(), "{bad}");
    }
}
