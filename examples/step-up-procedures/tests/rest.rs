//! Step-up on the REST binding, through the generated Rust client against
//! the real server: the refusal reads as `StepUpRequired`, one re-proof is
//! followed by exactly one retry, and the procedure runs once.

mod support;

use cratestack_client_rust::retry_after_step_up;
use step_up_procedures_example::clients::rest::cratestack_schema::{
    BalanceArgs, PayInvoiceArgs, procedures,
};
use step_up_procedures_example::clients::rest_client;
use step_up_procedures_example::flows::pay_over_rest;
use step_up_procedures_example::issuer::Authentication;
use support::{PAY_CHALLENGE, pay_step_up, service};

fn pay(invoice_id: &str) -> procedures::pay_invoice::Args {
    procedures::pay_invoice::Args {
        args: PayInvoiceArgs {
            invoiceId: invoice_id.to_owned(),
            amountCents: 4_200,
        },
    }
}

#[tokio::test]
async fn a_stale_password_session_steps_up_once_and_pays_once() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rest_client(&service.base_url, &session.device);

    let outcome = pay_over_rest(&client, &session, "inv-1")
        .await
        .expect("paid");
    assert_eq!(outcome.refusals, [pay_step_up()]);
    assert_eq!(outcome.value.invoiceId, "inv-1");
    assert_eq!(outcome.value.paidBy, "usr-ada");
    assert_eq!(service.ledger.paid(), ["inv-1"]);
}

#[tokio::test]
async fn a_fresh_hardware_key_session_pays_without_a_step_up() {
    let service = service().await;
    let session = service.session(Authentication::FRESH_HARDWARE_KEY);
    let client = rest_client(&service.base_url, &session.device);

    let outcome = pay_over_rest(&client, &session, "inv-1")
        .await
        .expect("paid");
    assert!(outcome.refusals.is_empty());
    assert_eq!(service.ledger.paid(), ["inv-1"]);
}

#[tokio::test]
async fn the_refusal_is_a_401_with_the_rfc_9470_challenge() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rest_client(&service.base_url, &session.device);

    let error = client
        .procedures()
        .pay_invoice(&pay("inv-1"), &[])
        .await
        .expect_err("a step-up refusal");
    assert_eq!(error.step_up(), Some(pay_step_up()));
    assert_eq!(error.challenges(), [PAY_CHALLENGE]);
    assert!(service.ledger.paid().is_empty(), "refused before it ran");
}

#[tokio::test]
async fn an_operation_without_a_requirement_takes_the_stale_session() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = rest_client(&service.base_url, &session.device);

    let args = procedures::balance::Args {
        args: BalanceArgs {
            account: "main".to_owned(),
        },
    };
    let balance = client
        .procedures()
        .balance(&args, &[])
        .await
        .expect("served");
    assert_eq!(balance.cents, 12_500);
}

#[tokio::test]
async fn a_declined_reproof_returns_the_refusal_without_a_retry() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = &rest_client(&service.base_url, &session.device);
    let args = &pay("inv-1");

    let result = retry_after_step_up(
        move || async move { client.procedures().pay_invoice(args, &[]).await },
        |_| std::future::ready(false),
    )
    .await;
    assert_eq!(result.expect_err("declined").step_up(), Some(pay_step_up()));
    assert!(service.ledger.paid().is_empty());
}

#[tokio::test]
async fn a_reproof_that_falls_short_is_not_retried_again() {
    let service = service().await;
    let session = service.session(Authentication::STALE_PASSWORD);
    let client = &rest_client(&service.base_url, &session.device);
    let args = &pay("inv-1");
    let mut reproofs = 0;

    // The user authenticates again, but with a password: fresh, still not
    // `hwk`. The helper sends once more, gets a second refusal, and stops.
    let result = retry_after_step_up(
        move || async move { client.procedures().pay_invoice(args, &[]).await },
        |_| {
            reproofs += 1;
            let fresh_password = Authentication {
                methods: &["pwd"],
                age_seconds: 0,
            };
            let token = service
                .issuer
                .mint("usr-ada", &session.device, fresh_password);
            session.device.use_token(token.expect("a token"));
            std::future::ready(true)
        },
    )
    .await;
    let step_up = result
        .expect_err("still refused")
        .step_up()
        .expect("a step-up");
    assert_eq!(step_up.unmet, ["amr"]);
    assert_eq!(reproofs, 1);
    assert!(service.ledger.paid().is_empty());
}
