//! Keyset-pagination predicates on [`TypeRef`], split from `model.rs` to keep
//! it under the file-length ceiling.

use super::TypeRef;

impl TypeRef {
    /// `CursorPage<T>` (`nextCursor` envelope), as opposed to offset `Page<T>`.
    pub fn is_cursor_page(&self) -> bool {
        self.name == "CursorPage"
    }
}
