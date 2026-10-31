//! What an app does with a step-up refusal, on each binding: call, and when
//! the server answers `StepUpRequired`, re-prove, then send the same call
//! once more through `retry_after_step_up`. The helper never loops: a
//! second refusal is returned, not retried.

use std::sync::Arc;

use cratestack_auth::AuthError;
use cratestack_client_rust::{ClientError, RpcClientError, StepUpRequired, retry_after_step_up};

use crate::clients::{rest, rpc};
use crate::device::Device;
use crate::issuer::{Authentication, Issuer};

/// A signed-in user on one device.
pub struct Session {
    /// The user, the id token's `sub`.
    pub subject: String,
    /// The device that signs every request.
    pub device: Arc<Device>,
    issuer: Arc<Issuer>,
}

impl Session {
    /// Signs `subject` in on `device`, authenticated `how`.
    pub fn sign_in(
        issuer: Arc<Issuer>,
        device: Arc<Device>,
        subject: &str,
        how: Authentication,
    ) -> Result<Self, AuthError> {
        device.use_token(issuer.mint(subject, &device, how)?);
        Ok(Self {
            subject: subject.to_owned(),
            device,
            issuer,
        })
    }

    /// The re-proof `retry_after_step_up` asks for. A real app would show a
    /// prompt naming what `step_up` requires (`amr`, `max_age`) and answer
    /// `false` when the user declines.
    pub fn reprove(&self, step_up: &StepUpRequired) -> bool {
        println!(
            "  step-up required: unmet {:?}, amr {:?}, max_age {:?}",
            step_up.unmet, step_up.amr, step_up.max_age
        );
        self.issuer.reprove(&self.subject, &self.device).is_ok()
    }
}

/// A call's result, and the step-up refusals met on the way (none when the
/// session already met the requirement, one when a re-proof was needed).
#[derive(Debug)]
pub struct Outcome<T> {
    /// What the call returned, after at most one retry.
    pub value: T,
    /// The refusals the re-proof answered.
    pub refusals: Vec<StepUpRequired>,
}

fn pay_args(invoice_id: &str) -> (String, i64) {
    (invoice_id.to_owned(), 4_200)
}

/// Pays `invoice_id` over REST.
pub async fn pay_over_rest(
    client: &rest::cratestack_schema::client::Client,
    session: &Session,
    invoice_id: &str,
) -> Result<Outcome<rest::cratestack_schema::Receipt>, ClientError> {
    let (invoice_id, amount_cents) = pay_args(invoice_id);
    let args = &rest::cratestack_schema::procedures::pay_invoice::Args {
        args: rest::cratestack_schema::PayInvoiceArgs {
            invoiceId: invoice_id,
            amountCents: amount_cents,
        },
    };
    let mut refusals = Vec::new();
    let value = retry_after_step_up(
        move || async move { client.procedures().pay_invoice(args, &[]).await },
        |step_up| {
            let proved = session.reprove(&step_up);
            refusals.push(step_up);
            std::future::ready(proved)
        },
    )
    .await?;
    Ok(Outcome { value, refusals })
}

/// Pays `invoice_id` over RPC, one unary call.
pub async fn pay_over_rpc(
    client: &rpc::cratestack_schema::client::Client,
    session: &Session,
    invoice_id: &str,
) -> Result<Outcome<rpc::cratestack_schema::Receipt>, RpcClientError> {
    let args = &rpc_pay_args(invoice_id);
    let mut refusals = Vec::new();
    let value = retry_after_step_up(
        move || client.procedures().pay_invoice(args),
        |step_up| {
            let proved = session.reprove(&step_up);
            refusals.push(step_up);
            std::future::ready(proved)
        },
    )
    .await?;
    Ok(Outcome { value, refusals })
}

/// Reads the balance and pays `invoice_id` in one RPC batch. The server
/// judges each frame on its own, so only the payment's frame is refused;
/// the retry sends the batch again after the re-proof.
pub async fn pay_in_batch(
    client: &rpc::cratestack_schema::client::Client,
    session: &Session,
    invoice_id: &str,
) -> Result<
    Outcome<(
        rpc::cratestack_schema::Balance,
        rpc::cratestack_schema::Receipt,
    )>,
    RpcClientError,
> {
    let pay = &rpc_pay_args(invoice_id);
    let mut refusals = Vec::new();
    let value = retry_after_step_up(
        move || send_batch(client, pay),
        |step_up| {
            let proved = session.reprove(&step_up);
            refusals.push(step_up);
            std::future::ready(proved)
        },
    )
    .await?;
    Ok(Outcome { value, refusals })
}

/// One `POST /rpc/batch` with a `balance` frame and a `payInvoice` frame.
pub async fn send_batch(
    client: &rpc::cratestack_schema::client::Client,
    pay: &rpc::cratestack_schema::procedures::pay_invoice::Args,
) -> Result<
    (
        rpc::cratestack_schema::Balance,
        rpc::cratestack_schema::Receipt,
    ),
    RpcClientError,
> {
    let balance = rpc::cratestack_schema::procedures::balance::Args {
        args: rpc::cratestack_schema::BalanceArgs {
            account: "main".to_owned(),
        },
    };
    let mut batch = client.batch();
    let balance = client.procedures().balance(&balance).queue(&mut batch);
    let receipt = client.procedures().pay_invoice(pay).queue(&mut batch);
    let mut results = batch.send().await?;
    let balance = results.take(balance);
    let receipt = results.take(receipt)?;
    Ok((balance?, receipt))
}

/// The arguments of `payInvoice` over RPC.
pub fn rpc_pay_args(invoice_id: &str) -> rpc::cratestack_schema::procedures::pay_invoice::Args {
    let (invoice_id, amount_cents) = pay_args(invoice_id);
    rpc::cratestack_schema::procedures::pay_invoice::Args {
        args: rpc::cratestack_schema::PayInvoiceArgs {
            invoiceId: invoice_id,
            amountCents: amount_cents,
        },
    }
}
