//! A provider that records the canonical request it is handed (ADR 0021
//! §3, cratestack#1243) and then refuses, so no handler runs.

use std::sync::{Arc, Mutex};

use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext};

#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct Recorder(Arc<Mutex<Vec<Seen>>>);

impl Recorder {
    /// The one request the provider saw: every request is authenticated
    /// exactly once, a batch included.
    pub fn only(&self) -> Seen {
        let seen = self.0.lock().unwrap();
        assert_eq!(seen.len(), 1, "one provider call per request: {seen:?}");
        seen[0].clone()
    }
}

impl AuthProvider for Recorder {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        self.0.lock().unwrap().push(Seen {
            method: request.method.to_owned(),
            path: request.path.to_owned(),
            query: request.query.map(str::to_owned),
            body: request.body.to_vec(),
        });
        Err(CratestackError::Unauthorized("recorded".into()))
    }
}
