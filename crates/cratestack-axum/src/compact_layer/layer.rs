//! [`CompactLayer`]: the tower layer and its configuration.

use std::sync::Arc;

use cratestack_core::CratestackError;
use cratestack_core::compact::KeyTables;
use tower::Layer;

use super::service::CompactService;

/// The compact-keys layer; see the [module docs](super).
#[derive(Clone)]
pub struct CompactLayer {
    pub(super) tables: Arc<KeyTables>,
    pub(super) body_limit: usize,
}

impl CompactLayer {
    /// A layer over `tables`. `body_limit_bytes` bounds a compact request
    /// body and the named body it expands to (pass the router's own limit).
    pub fn new(tables: KeyTables, body_limit_bytes: usize) -> Self {
        Self {
            tables: Arc::new(tables),
            body_limit: body_limit_bytes,
        }
    }

    /// [`new`](Self::new) over a [`KeyTables::to_bytes`] artifact, such as
    /// the generated `KEY_TABLES` constant.
    pub fn from_artifact(
        artifact: &[u8],
        body_limit_bytes: usize,
    ) -> Result<Self, CratestackError> {
        let tables = KeyTables::from_bytes(artifact)
            .map_err(|error| CratestackError::Internal(format!("key tables: {error}")))?;
        Ok(Self::new(tables, body_limit_bytes))
    }
}

impl<S> Layer<S> for CompactLayer {
    type Service = CompactService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CompactService {
            inner,
            layer: self.clone(),
        }
    }
}
