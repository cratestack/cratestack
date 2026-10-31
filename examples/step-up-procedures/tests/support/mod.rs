//! The service on a loopback port, and a user signed in on one device.

#![allow(dead_code)]

use std::sync::Arc;

use cratestack_client_rust::StepUpRequired;
use step_up_procedures_example::device::Device;
use step_up_procedures_example::flows::Session;
use step_up_procedures_example::issuer::{Authentication, Issuer};
use step_up_procedures_example::server::{Ledger, spawn};
use url::Url;

/// A running service and what the tests inspect.
pub struct Service {
    pub base_url: Url,
    pub issuer: Arc<Issuer>,
    pub ledger: Ledger,
    _server: tokio::task::JoinHandle<()>,
}

pub async fn service() -> Service {
    let issuer = Arc::new(Issuer::new());
    let ledger = Ledger::default();
    let (addr, server) = spawn(&issuer, ledger.clone(), "127.0.0.1:0")
        .await
        .expect("the service binds");
    Service {
        base_url: Url::parse(&format!("http://{addr}/")).expect("a URL"),
        issuer,
        ledger,
        _server: server,
    }
}

impl Service {
    /// `usr-ada` signed in on a new device, authenticated `how`.
    pub fn session(&self, how: Authentication) -> Session {
        let device = Arc::new(Device::new("device-1"));
        Session::sign_in(self.issuer.clone(), device, "usr-ada", how).expect("a token")
    }
}

/// What `payInvoice` asks of a stale password session: both its freshness
/// and its method are unmet. It declares no `acr`.
pub fn pay_step_up() -> StepUpRequired {
    StepUpRequired {
        max_age: Some(300),
        acr_values: Vec::new(),
        amr: vec!["hwk".to_owned()],
        unmet: vec!["max_age".to_owned(), "amr".to_owned()],
    }
}

/// The RFC 9470 challenge REST sends with the refusal.
pub const PAY_CHALLENGE: &str = r#"Bearer error="insufficient_user_authentication", error_description="step-up authentication required", max_age="300""#;
