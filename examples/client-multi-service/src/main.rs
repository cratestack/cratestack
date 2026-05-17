//! BFF / orchestrator pattern: one binary that fans out to **two** upstream
//! CrateStack services, each contributing its own typed client surface
//! generated from its own `.cstack` schema.
//!
//! Both `include_client_schema!` invocations live inside their own modules
//! so the emitted `cratestack_schema` modules don't collide; each module
//! re-exports the bits this binary uses.
//!
//! ### Run
//!
//! ```bash
//! BILLING_URL=http://billing.internal:3000 \
//! INVENTORY_URL=http://inventory.internal:3000 \
//! cargo run -p client-multi-service-example
//! ```
//!
//! Without either env var, the example prints the generated typed surface
//! for both upstream services and exits.

use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use url::Url;

pub mod billing {
    use cratestack::include_client_schema;
    include_client_schema!("schemas/billing.cstack");
}

pub mod inventory {
    use cratestack::include_client_schema;
    include_client_schema!("schemas/inventory.cstack");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let billing_url = std::env::var("BILLING_URL").ok();
    let inventory_url = std::env::var("INVENTORY_URL").ok();

    if billing_url.is_none() || inventory_url.is_none() {
        print_surface();
        return Ok(());
    }

    let billing_url = Url::parse(&billing_url.unwrap())?;
    let inventory_url = Url::parse(&inventory_url.unwrap())?;

    let billing_runtime = CratestackClient::new(ClientConfig::new(billing_url), CborCodec);
    let inventory_runtime = CratestackClient::new(ClientConfig::new(inventory_url), CborCodec);

    let billing = billing::cratestack_schema::client::Client::new(billing_runtime);
    let inventory = inventory::cratestack_schema::client::Client::new(inventory_runtime);

    // Fan out concurrently — typical BFF pattern. The intermediate
    // `invoices()` / `products()` clients are borrows, so we bind them to
    // named locals before awaiting.
    let invoices_client = billing.invoices();
    let products_client = inventory.products();
    let (invoices, products) = tokio::try_join!(
        invoices_client.list(&[("limit", "5")], &[]),
        products_client.list(&[("limit", "5")], &[]),
    )?;

    println!("billing returned {} invoices", invoices.len());
    for invoice in invoices.iter().take(3) {
        println!(
            "  invoice #{:<4} customer={:<4} amount={:<8} paid={}",
            invoice.id, invoice.customerId, invoice.amountCents, invoice.paid
        );
    }
    println!("inventory returned {} products", products.len());
    for product in products.iter().take(3) {
        println!(
            "  product #{:<4} sku={:<10} name={:<20} on_hand={}",
            product.id, product.sku, product.name, product.stockOnHand
        );
    }
    Ok(())
}

fn print_surface() {
    println!("BILLING_URL / INVENTORY_URL not set. Generated typed surfaces:");
    println!(
        "  billing   models = {:?}",
        billing::cratestack_schema::MODELS
    );
    println!(
        "  inventory models = {:?}",
        inventory::cratestack_schema::MODELS
    );
    println!();
    println!("Set BILLING_URL=… and INVENTORY_URL=… to fan out to live services.");
}
