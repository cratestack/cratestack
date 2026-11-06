use super::{FlutterCallBinding, FlutterCoseError};
use cratestack_cose::CallBinding;

impl FlutterCallBinding {
    /// The owned binding, addressed to `audience`.
    pub(crate) fn with_audience(self, audience: &str) -> Result<CallBinding, FlutterCoseError> {
        let contract_sha = self
            .contract_sha
            .as_slice()
            .try_into()
            .map_err(|_| FlutterCoseError::misuse("the op contract digest is 32 bytes"))?;
        let call = CallBinding {
            audience: audience.to_owned(),
            method: self.method,
            route: self.route,
            path_params: self.path_params,
            query: self.query,
            contract_sha,
            idempotency_key: self.idempotency_key,
            if_match: self.if_match,
            payload_type: self.payload_type,
            payload_accept: self.payload_accept,
        };
        call.validate()?;
        Ok(call)
    }
}
