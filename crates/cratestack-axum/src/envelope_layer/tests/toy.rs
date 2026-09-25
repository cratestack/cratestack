//! A deliberately trivial `ServerEnvelope` (no cryptography at all) that
//! records how the layer drives it: the plug-in and hostile-plug-in tests
//! need to see which bindings it was handed and to make it fail on demand.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, VerifiedSigner};

use crate::envelope_layer::{OpenedRequest, ServerEnvelope};

pub const TOY: &str = "application/x-toy";

#[derive(Clone, Default)]
pub struct Toy {
    /// Recognise `application/x-toy` through `is_envelope_content_type`.
    pub claims_toy: bool,
    /// What `open_request` fails with instead of opening, if set.
    pub open_error: Option<fn() -> CratestackError>,
    pub seal_fails: bool,
    /// `(route, path params)` of every binding `open_request` saw.
    pub opened: Arc<Mutex<Vec<(String, Vec<String>)>>>,
}

impl Toy {
    pub fn opens(&self) -> usize {
        self.opened.lock().expect("lock").len()
    }
}

#[async_trait]
impl ServerEnvelope for Toy {
    fn media_type(&self) -> &'static str {
        TOY
    }

    fn is_envelope_content_type(&self, content_type: &str) -> bool {
        self.claims_toy && content_type.starts_with(TOY)
    }

    /// Opens any body that starts with `TOY:`.
    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError> {
        let params = bind.path_params.iter().map(str::to_owned).collect();
        self.opened
            .lock()
            .expect("lock")
            .push((bind.route.to_string(), params));
        if let Some(error) = self.open_error {
            return Err(error());
        }
        if !body.starts_with(b"TOY:") {
            return Err(CratestackError::Unauthorized(
                "toy: bad magic at byte 0".to_owned(),
            ));
        }
        let signer = VerifiedSigner::new(vec![0; 8], [0x42; 32], 0);
        Ok(OpenedRequest::new(body.slice(4..), signer))
    }

    /// `TOYRESP:<route>:<status>:<payload>`, so a test can read what was
    /// bound.
    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError> {
        if self.seal_fails {
            return Err(CratestackError::Internal("hsm down".to_owned()));
        }
        let status = bind.response.map_or(0, |response| response.status);
        let mut out = format!("TOYRESP:{}:{status}:", bind.route).into_bytes();
        out.extend_from_slice(payload);
        Ok(out.into())
    }
}

pub fn toy_request(
    method: http::Method,
    uri: &str,
    content_type: &str,
    body: &[u8],
) -> axum::extract::Request {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(http::header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body.to_vec()))
        .expect("request")
}
