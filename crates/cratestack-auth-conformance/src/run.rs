//! [`Conformance`]: the cases a provider author declares, and the run that
//! drives each through every transport and mount.

use std::sync::Arc;

use cratestack::axum::Router;
use cratestack::{AuthProvider, RequestContext};

use crate::apps;
use crate::canonical::{Canonical, Recording, Transport};
use crate::check;
use crate::credentials::Credentials;
use crate::expect::Expected;
use crate::send::{self, Answer};

type Signer = Arc<dyn Fn(&Canonical) -> Credentials + Send + Sync>;

enum Case {
    Accept {
        subject: String,
        sign: Signer,
    },
    Refuse {
        name: String,
        sign: Signer,
        expected: Expected,
    },
}

/// Starts a conformance run for `provider`. Declare at least one case with
/// [`Conformance::accepts`] or [`Conformance::refuses`], then
/// [`run`](Conformance::run) it.
pub fn auth_conformance<P: AuthProvider>(provider: P) -> Conformance<P> {
    Conformance {
        provider,
        cases: Vec::new(),
    }
}

/// A provider and the cases it must pass on every transport.
pub struct Conformance<P> {
    provider: P,
    cases: Vec<Case>,
}

const MOUNTS: [&str; 2] = ["", "/kit"];
const TRANSPORTS: [Transport; 4] = [
    Transport::Rest,
    Transport::RpcUnary,
    Transport::RpcBatch,
    Transport::Mcp,
];

impl<P: AuthProvider> Conformance<P> {
    /// `sign` returns a credential the provider accepts for this exact
    /// request (headers, or [`Credentials`] with extensions); the call must
    /// succeed everywhere as `subject`.
    #[must_use]
    pub fn accepts<C: Into<Credentials>>(
        mut self,
        subject: impl Into<String>,
        sign: impl Fn(&Canonical) -> C + Send + Sync + 'static,
    ) -> Self {
        let subject = subject.into();
        self.cases.push(Case::Accept {
            subject,
            sign: Arc::new(move |canonical| sign(canonical).into()),
        });
        self
    }

    /// `sign` returns a credential the provider refuses; the refusal must
    /// read as `expected` everywhere. `name` labels the case in the report.
    #[must_use]
    pub fn refuses<C: Into<Credentials>>(
        mut self,
        name: impl Into<String>,
        sign: impl Fn(&Canonical) -> C + Send + Sync + 'static,
        expected: Expected,
    ) -> Self {
        self.cases.push(Case::Refuse {
            name: name.into(),
            sign: Arc::new(move |canonical| sign(canonical).into()),
            expected,
        });
        self
    }

    /// Drives every case through REST, RPC unary, RPC batch (each at the
    /// root and under `/kit`) and MCP, and panics with every finding when
    /// the provider does not conform.
    pub async fn run(self) {
        assert!(!self.cases.is_empty(), "declare at least one case");
        let recording = Recording::new(self.provider);
        let mut findings = Vec::new();
        for transport in TRANSPORTS {
            for mount in MOUNTS {
                if transport == Transport::Mcp && !mount.is_empty() {
                    continue;
                }
                let app = app(&recording, transport, mount);
                for case in &self.cases {
                    let canonical = send::canonical(transport, mount);
                    let (label, found) = run_case(&recording, &app, &canonical, case).await;
                    let place = format!("{transport:?} at {:?}, {label}", mount);
                    findings.extend(found.into_iter().map(|f| format!("{place}: {f}")));
                }
            }
        }
        assert!(
            findings.is_empty(),
            "the provider does not conform (ADR 0021 §6), {} finding(s):\n- {}",
            findings.len(),
            findings.join("\n- ")
        );
    }
}

fn app<P: AuthProvider>(provider: &Recording<P>, transport: Transport, mount: &str) -> Router {
    match transport {
        Transport::Rest => apps::rest(provider.clone(), mount),
        Transport::Mcp => apps::mcp(provider.clone()),
        _ => apps::rpc(provider.clone(), mount),
    }
}

async fn run_case<P: AuthProvider>(
    provider: &Recording<P>,
    app: &Router,
    canonical: &Canonical,
    case: &Case,
) -> (String, Vec<String>) {
    let (label, sign) = match case {
        Case::Accept { subject, sign } => (format!("accepting {subject:?}"), sign),
        Case::Refuse { name, sign, .. } => (format!("refusing {name:?}"), sign),
    };
    let credential = sign(canonical);
    let absent = canonical.transport == Transport::Mcp
        && provider_says_absent(provider, canonical, &credential);
    let answer: Answer = send::send(app, canonical, credential).await;
    let seen = provider.take();
    let mut found = check::canonical(canonical, &seen, usize::from(!absent));
    found.extend(match case {
        _ if absent => check::absent_on_mcp(&answer),
        Case::Accept { subject, .. } => check::accepted(canonical.transport, &answer, subject),
        Case::Refuse { expected, .. } => check::refused(canonical.transport, &answer, expected),
    });
    (label, found)
}

/// Whether the provider's `credential` calls this request's credential
/// absent, as MCP's guard asks it (the request head, no body).
fn provider_says_absent<P: AuthProvider>(
    provider: &Recording<P>,
    canonical: &Canonical,
    credentials: &Credentials,
) -> bool {
    let request = RequestContext {
        method: canonical.method,
        path: canonical.path,
        query: canonical.query,
        headers: &credentials.headers,
        body: &[],
        extensions: &credentials.extensions,
    };
    provider.credential(&request).is_absent()
}
