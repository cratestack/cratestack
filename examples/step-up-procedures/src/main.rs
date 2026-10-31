//! Runs the service on a loopback port and drives it with the generated
//! clients: a stale password session reads the balance, meets the step-up
//! refusal on `payInvoice`, re-proves with the hardware key and pays, on
//! REST, on an RPC unary call and in an RPC batch.
//!
//! ```bash
//! cargo run -p step-up-procedures-example
//! ```

use std::sync::Arc;

use step_up_procedures_example::clients::{rest_client, rpc_client};
use step_up_procedures_example::device::Device;
use step_up_procedures_example::flows::{Session, pay_in_batch, pay_over_rest, pay_over_rpc};
use step_up_procedures_example::issuer::{Authentication, Issuer};
use step_up_procedures_example::server::{Ledger, spawn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let issuer = Arc::new(Issuer::new());
    let ledger = Ledger::default();
    let (addr, _server) = spawn(&issuer, ledger.clone(), "127.0.0.1:0").await?;
    let base_url = url::Url::parse(&format!("http://{addr}/"))?;
    println!("serving REST and RPC on {base_url}");

    for (binding, invoice) in [
        ("REST", "inv-1"),
        ("RPC unary", "inv-2"),
        ("RPC batch", "inv-3"),
    ] {
        // A fresh password session, twenty minutes old, for each binding.
        let device = Arc::new(Device::new("device-1"));
        let session = Session::sign_in(
            issuer.clone(),
            device.clone(),
            "usr-ada",
            Authentication::STALE_PASSWORD,
        )?;
        println!("\n{binding}: pay {invoice} with a 20-minute-old password session");
        match binding {
            "REST" => {
                let outcome =
                    pay_over_rest(&rest_client(&base_url, &device), &session, invoice).await?;
                println!(
                    "  paid after {} step-up: {:?}",
                    outcome.refusals.len(),
                    outcome.value
                );
            }
            "RPC unary" => {
                let outcome =
                    pay_over_rpc(&rpc_client(&base_url, &device), &session, invoice).await?;
                println!(
                    "  paid after {} step-up: {:?}",
                    outcome.refusals.len(),
                    outcome.value
                );
            }
            _ => {
                let outcome =
                    pay_in_batch(&rpc_client(&base_url, &device), &session, invoice).await?;
                println!(
                    "  paid after {} step-up: {:?}",
                    outcome.refusals.len(),
                    outcome.value
                );
            }
        }
    }
    println!("\nledger: {:?}", ledger.paid());
    Ok(())
}
