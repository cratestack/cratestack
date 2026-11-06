//! Errors of the key-table derivation and the artifact reader.

/// Why a key table could not be built or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CompactError {
    /// The op contract JSON is not a canonical op contract.
    #[error("malformed op contract: {0}")]
    Contract(String),
    /// The artifact bytes are not a key-table artifact this build reads.
    #[error("malformed key-table artifact: {0}")]
    Artifact(String),
}
