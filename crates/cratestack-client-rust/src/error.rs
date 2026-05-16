use cratestack_core::{CoolError, CoolErrorResponse};
use reqwest::StatusCode;

pub type HeaderPair<'a> = (&'a str, &'a str);
pub type QueryPair<'a> = (&'a str, &'a str);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("codec error: {0}")]
    Codec(#[from] CoolError),
    #[error("state error: {0}")]
    State(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("remote call failed with status {status}: {message}")]
    Remote {
        status: StatusCode,
        error: Option<CoolErrorResponse>,
        message: String,
    },
}
