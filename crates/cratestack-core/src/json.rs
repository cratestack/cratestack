//! Schema-declared `Json` columns need a model-struct field type that's the
//! same on every backend so the same struct compiles on server and on
//! embedded (including `wasm32-unknown-unknown`, which can't depend on sqlx).
//!
//! This newtype lives in `cratestack-core` so every backend can reference it
//! without introducing a cyclic dep. `cratestack-sqlx` provides the sqlx
//! `Type` / `Encode` / `Decode` impls on native targets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Json<T>(pub T);

impl<T> Json<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> std::ops::Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> From<T> for Json<T> {
    fn from(value: T) -> Self {
        Json(value)
    }
}
