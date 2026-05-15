//! `cratestack-studio-generator` is in transition.
//!
//! The 0.3 line shipped a multi-crate Leptos+Axum scaffold generator driven
//! by Jinja templates. That has been removed. The replacement is
//! [`cratestack-studio`], a single binary served from a `studio.toml`
//! workspace file.
//!
//! In Phase 2 this crate becomes a thin `eject` step: it will copy
//! `cratestack-studio`'s own sources into an output directory so callers
//! can fork the UI without losing a stable upgrade path. Until then the
//! [`eject`] function returns [`EjectError::NotImplemented`].

#[derive(Debug, thiserror::Error)]
pub enum EjectError {
    #[error(
        "studio eject is not implemented yet; this lands in Phase 2 of the studio rewrite"
    )]
    NotImplemented,
}

/// Phase 0 placeholder. Always fails with [`EjectError::NotImplemented`].
pub fn eject() -> Result<(), EjectError> {
    Err(EjectError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eject_is_not_implemented() {
        assert!(matches!(eject(), Err(EjectError::NotImplemented)));
    }
}
