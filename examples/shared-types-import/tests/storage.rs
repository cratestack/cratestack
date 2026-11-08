//! An import shares a shape and each importer owns the storage. The imported
//! enum is a column of each service's own table, and the mixin's fields are
//! columns too.

use cratestack_rusqlite::ModelDelegate;
use shared_types_import_example::billing::cratestack_schema as billing_schema;
use shared_types_import_example::shipping::cratestack_schema as shipping_schema;
use shared_types_import_example::{billing, shipping};

use billing_schema::{CreateInvoiceInput, INVOICE_MODEL};
use shipping_schema::{CreateShipmentInput, SHIPMENT_MODEL, ShipmentStatus};

fn invoice(id: i64, order_id: i64, currency: billing_schema::Currency) -> CreateInvoiceInput {
    CreateInvoiceInput {
        id,
        orderId: order_id,
        customer: "Ada".to_owned(),
        totalMinor: 12_500,
        currency,
        paid: false,
    }
}

fn shipment(
    id: i64,
    order_id: i64,
    currency: shipping_schema::Currency,
    status: ShipmentStatus,
) -> CreateShipmentInput {
    CreateShipmentInput {
        id,
        orderId: order_id,
        destination: "Douala".to_owned(),
        status,
        insuredMinor: 12_500,
        insuredCurrency: currency,
    }
}

#[test]
fn billing_stores_the_imported_enum_and_mixin_in_its_own_table() {
    let db = billing::open_in_memory().expect("billing database opens");
    let invoices = ModelDelegate::new(&db, &INVOICE_MODEL);
    let created = invoices
        .create(invoice(1, 42, billing_schema::Currency::XAF))
        .run()
        .expect("create succeeds");
    assert_eq!(created.currency, billing_schema::Currency::XAF);

    let found = invoices
        .find_unique(1)
        .run()
        .expect("find succeeds")
        .expect("row exists");
    assert_eq!(found.currency, billing_schema::Currency::XAF);
    // Both mixin fields came from `shared/common.cstack`; `createdAt` was
    // filled by its `@default(now())`.
    assert_eq!(found.orderId, 42);
    assert!(found.createdAt <= cratestack::chrono::Utc::now());

    // Stored by name, in billing's own table.
    let sql = format!(
        "SELECT typeof(currency), currency FROM {} WHERE id = ?1",
        INVOICE_MODEL.table_name
    );
    let stored: (String, String) = db
        .with_connection(|conn| {
            Ok(conn
                .query_row(&sql, [1_i64], |row| Ok((row.get(0)?, row.get(1)?)))
                .expect("row stored"))
        })
        .unwrap();
    assert_eq!(stored, ("text".to_owned(), "XAF".to_owned()));

    let total = billing::total(&found);
    assert_eq!(total.amountMinor, 12_500);
    assert_eq!(total.currency, billing_schema::Currency::XAF);
}

#[test]
fn shipping_filters_on_the_imported_enum_and_on_its_own() {
    use shipping_schema::Currency::{EUR, XAF};

    let db = shipping::open_in_memory().expect("shipping database opens");
    let shipments = ModelDelegate::new(&db, &SHIPMENT_MODEL);
    for input in [
        shipment(1, 41, EUR, ShipmentStatus::Delivered),
        shipment(2, 42, XAF, ShipmentStatus::InTransit),
        shipment(3, 43, XAF, ShipmentStatus::Delivered),
    ] {
        shipments.create(input).run().expect("create succeeds");
    }

    let ids = |found: Vec<shipping_schema::models::Shipment>| {
        let mut ids: Vec<i64> = found.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids
    };
    let in_xaf = shipments
        .find_many()
        .where_(shipping_schema::shipment::insuredCurrency().eq(XAF))
        .run()
        .expect("filter on the imported enum");
    assert_eq!(ids(in_xaf), [2, 3]);

    let delivered = shipments
        .find_many()
        .where_(shipping_schema::shipment::status().eq(ShipmentStatus::Delivered))
        .run()
        .expect("filter on the service's own enum");
    assert_eq!(ids(delivered), [1, 3]);
}

/// The mixin field is the key the two services share: one order, found in
/// each service's own database by the same `orderId`.
#[test]
fn one_order_is_found_in_both_services_by_the_mixin_field() {
    let billing_db = billing::open_in_memory().unwrap();
    let invoices = ModelDelegate::new(&billing_db, &INVOICE_MODEL);
    invoices
        .create(invoice(1, 42, billing_schema::Currency::EUR))
        .run()
        .unwrap();
    invoices
        .create(invoice(2, 99, billing_schema::Currency::EUR))
        .run()
        .unwrap();

    let shipping_db = shipping::open_in_memory().unwrap();
    let shipments = ModelDelegate::new(&shipping_db, &SHIPMENT_MODEL);
    shipments
        .create(shipment(
            7,
            42,
            shipping_schema::Currency::EUR,
            ShipmentStatus::Pending,
        ))
        .run()
        .unwrap();

    let billed = invoices
        .find_many()
        .where_(billing_schema::invoice::orderId().eq(42_i64))
        .run()
        .unwrap();
    let shipped = shipments
        .find_many()
        .where_(shipping_schema::shipment::orderId().eq(42_i64))
        .run()
        .unwrap();
    assert_eq!(billed.iter().map(|i| i.id).collect::<Vec<_>>(), [1]);
    assert_eq!(shipped.iter().map(|s| s.id).collect::<Vec<_>>(), [7]);
    assert_eq!(
        billing::total(&billed[0]).amountMinor,
        shipping::insured_value(&shipped[0]).amountMinor
    );
}
