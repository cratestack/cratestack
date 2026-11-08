//! Writes one order into both services, an invoice in billing's database and
//! a shipment in shipping's, and prints each in the shared `Money` shape.

use std::error::Error;

use cratestack_rusqlite::ModelDelegate;
use shared_types_import_example::billing::cratestack_schema as billing_schema;
use shared_types_import_example::shipping::cratestack_schema as shipping_schema;
use shared_types_import_example::{billing, shipping};

fn main() -> Result<(), Box<dyn Error>> {
    let billing_db = billing::open_in_memory()?;
    let invoices = ModelDelegate::new(&billing_db, &billing_schema::INVOICE_MODEL);
    let invoice = invoices
        .create(billing_schema::CreateInvoiceInput {
            id: 1,
            orderId: 42,
            customer: "Ada".to_owned(),
            totalMinor: 12_500,
            currency: billing_schema::Currency::XAF,
            paid: false,
        })
        .run()?;

    let shipping_db = shipping::open_in_memory()?;
    let shipments = ModelDelegate::new(&shipping_db, &shipping_schema::SHIPMENT_MODEL);
    let shipment = shipments
        .create(shipping_schema::CreateShipmentInput {
            id: 7,
            orderId: 42,
            destination: "Douala".to_owned(),
            status: shipping_schema::ShipmentStatus::Pending,
            insuredMinor: 12_500,
            insuredCurrency: shipping_schema::Currency::XAF,
        })
        .run()?;

    println!(
        "billing:  invoice {} for order {}, total {:?}",
        invoice.id,
        invoice.orderId,
        billing::total(&invoice)
    );
    println!(
        "shipping: shipment {} for order {} ({}), insured {:?}",
        shipment.id,
        shipment.orderId,
        shipment.status,
        shipping::insured_value(&shipment)
    );
    Ok(())
}
