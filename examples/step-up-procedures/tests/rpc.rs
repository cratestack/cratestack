//! Step-up on the RPC binding: a unary call, and a batch whose frames the
//! server judges one by one, so only the payment's frame is refused.

mod support;

use cratestack_client_rust::retry_after_step_up;
use step_up_procedures_example::clients::rpc::cratestack_schema::{BalanceArgs, procedures};
use step_up_procedures_example::clients::rpc_client;
use step_up_procedures_example::flows::{pay_in_batch, pay_over_rpc, rpc_pay_args};
use step_up_procedures_example::issuer::Authentication;
use support::{pay_step_up, service};

#[tokio::test]
async fn a_unary_call_steps_up_once_and_pays_once() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rpc_client(&service.base_url, &session.device);

    let outcome = pay_over_rpc(&client, &session, "inv-2")
        .await
        .expect("paid");
    assert_eq!(outcome.refusals, [pay_step_up()]);
    assert_eq!(outcome.value.paidBy, "usr-ada");
    assert_eq!(service.ledger.paid(), ["inv-2"]);
}

#[tokio::test]
async fn a_unary_refusal_is_unauthenticated_and_typed() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rpc_client(&service.base_url, &session.device);

    let error = client
        .procedures()
        .pay_invoice(&rpc_pay_args("inv-2"))
        .await
        .expect_err("a step-up refusal");
    let remote = error.remote().expect("a remote error");
    assert_eq!(remote.body.code, "unauthenticated");
    assert_eq!(error.step_up(), Some(pay_step_up()));
    assert!(service.ledger.paid().is_empty());
}

#[tokio::test]
async fn a_batch_refuses_only_the_frame_that_needs_a_step_up() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rpc_client(&service.base_url, &session.device);

    let mut batch = client.batch();
    let balance = client
        .procedures()
        .balance(&procedures::balance::Args {
            args: BalanceArgs {
                account: "main".to_owned(),
            },
        })
        .queue(&mut batch);
    let pay = client
        .procedures()
        .pay_invoice(&rpc_pay_args("inv-3"))
        .queue(&mut batch);
    let mut results = batch.send().await.expect("the batch is answered");
    assert_eq!(results.take(balance).expect("balance runs").cents, 12_500);
    let refusal = results.take(pay).expect_err("payInvoice is refused");
    assert_eq!(refusal.step_up(), Some(pay_step_up()));
    assert!(service.ledger.paid().is_empty());
}

#[tokio::test]
async fn a_batch_is_sent_again_once_after_the_reproof() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rpc_client(&service.base_url, &session.device);

    let outcome = pay_in_batch(&client, &session, "inv-3")
        .await
        .expect("paid");
    assert_eq!(outcome.refusals, [pay_step_up()]);
    let (balance, receipt) = outcome.value;
    assert_eq!(balance.cents, 12_500);
    assert_eq!(receipt.invoiceId, "inv-3");
    assert_eq!(service.ledger.paid(), ["inv-3"]);
}

#[tokio::test]
async fn a_declined_reproof_returns_the_refusal_without_a_retry() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rpc_client(&service.base_url, &session.device);
    let args = &rpc_pay_args("inv-2");
    let mut sends = 0;

    let result = retry_after_step_up(
        || {
            sends += 1;
            client.procedures().pay_invoice(args)
        },
        |_| std::future::ready(false),
    )
    .await;
    assert_eq!(result.expect_err("declined").step_up(), Some(pay_step_up()));
    assert_eq!(sends, 1);
    assert!(service.ledger.paid().is_empty());
}
