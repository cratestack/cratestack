//! The payment fixture the shared vectors are built on.
//!
//! ADR 0006's measurements use "a 7-field payment row: 112 B as CBOR with
//! string keys". The proof of concept that produced them is not in this
//! repository, so the row is reconstructed: the field names and types are
//! what a payment row plausibly holds, and the two free-text values were
//! chosen so that `CborCodec` encodes the row to exactly 112 bytes:
//!
//! | field        | value                                   | encoded (key + value) |
//! |--------------|-----------------------------------------|----------------------:|
//! | (map head)   | 7 entries                               |  1 |
//! | `id`         | UUID `0192f5a8-7c3e-7b21-9d4f-6a1e2c3b4d5e` (a 16-byte `bstr`: `CborCodec` is not human-readable) | 3 + 17 = 20 |
//! | `payer`      | `"Amina Tchoupo"` (13 chars)            | 6 + 14 = 20 |
//! | `amount`     | `125000` (minor units, `uint32`)        | 7 + 5 = 12 |
//! | `currency`   | `"XAF"`                                 | 9 + 4 = 13 |
//! | `status`     | `"settled"`                             | 7 + 8 = 15 |
//! | `created_at` | `1790000000` (Unix seconds, `uint32`)   | 11 + 5 = 16 |
//! | `note`       | `Some("rent sept")` (9 chars)           | 5 + 10 = 15 |
//! | **total**    |                                         | **112** |

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub id: uuid::Uuid,
    pub payer: String,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub created_at: i64,
    pub note: Option<String>,
}

pub fn payment() -> Payment {
    Payment {
        id: uuid::Uuid::parse_str("0192f5a8-7c3e-7b21-9d4f-6a1e2c3b4d5e").expect("uuid"),
        payer: "Amina Tchoupo".to_owned(),
        amount: 125_000,
        currency: "XAF".to_owned(),
        status: "settled".to_owned(),
        created_at: 1_790_000_000,
        note: Some("rent sept".to_owned()),
    }
}

pub fn payment_bytes() -> Vec<u8> {
    CborCodec.encode(&payment()).expect("encode payment")
}
