//! Why a rename was refused, and the wording the editor shows for it.
//!
//! Split out of [`crate::rename`] so that module stays focused on computing
//! edits. The wording is the whole value here: refusals travel as request
//! errors rather than as an empty edit, because a rename that silently does
//! nothing is worse than one that says why it declined — and a message that
//! only says "cannot rename" leaves the author guessing which of five
//! reasons applied.

use tower_lsp_server::jsonrpc;

#[derive(Debug)]
pub(crate) enum RenameError {
    /// The cursor is not on a symbol this schema declares. Notably includes
    /// builtin scalars: `String` resolves as a type reference, but nothing
    /// declares it, and rewriting every `String` in a file is not a rename.
    NotRenameable,
    /// The schema on hand predates the buffer, so its spans no longer describe
    /// the text that would be edited.
    StaleSchema,
    InvalidIdentifier(String),
    /// The new name is already taken in the scope the symbol lives in.
    Conflict(String),
    /// The new name is a keyword or a builtin type, either of which changes how
    /// the file parses rather than just what it calls something.
    Reserved(String),
}

/// Refusals travel as request errors, not as an empty edit: a rename that
/// silently does nothing is worse than one that says why it declined.
impl From<RenameError> for jsonrpc::Error {
    fn from(error: RenameError) -> Self {
        Self {
            code: jsonrpc::ErrorCode::InvalidRequest,
            message: error.message().into(),
            data: None,
        }
    }
}

impl RenameError {
    fn message(&self) -> String {
        match self {
            Self::NotRenameable => "Only names declared in this schema can be renamed.".to_owned(),
            Self::StaleSchema => concat!(
                "This file has a syntax error, so the language server is working from an older ",
                "version of it. Fix the error before renaming — edits computed here would apply ",
                "at the wrong positions."
            )
            .to_owned(),
            Self::InvalidIdentifier(name) => {
                format!("`{name}` is not a valid identifier.")
            }
            Self::Conflict(name) => format!("`{name}` is already declared here."),
            Self::Reserved(name) => {
                format!("`{name}` is a reserved word or builtin type.")
            }
        }
    }
}
