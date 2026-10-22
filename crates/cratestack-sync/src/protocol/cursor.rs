//! The pull cursor: a position in the server's commit-ordered change log.
//!
//! A position is `(txid, seq)` — the Postgres 64-bit transaction id that
//! wrote the change, then a per-change sequence number to order changes
//! within one transaction. The server only ever hands out positions below
//! the oldest still-running transaction (`pg_snapshot_xmin`), which is what
//! makes "everything after my cursor" safe to ask for: no transaction can
//! later commit *behind* a cursor a client already holds. A plain sequence
//! or timestamp cursor gets this wrong — see the design doc, §4.
//!
//! Clients treat the cursor as opaque and only echo it back.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cursor {
    pub txid: u64,
    pub seq: i64,
}

impl Cursor {
    /// The position before every change: a fresh client starts here.
    pub const ZERO: Cursor = Cursor { txid: 0, seq: 0 };

    pub fn new(txid: u64, seq: i64) -> Self {
        Self { txid, seq }
    }

    pub fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.txid, self.seq)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed sync cursor `{0}`")]
pub struct CursorParseError(pub String);

impl FromStr for Cursor {
    type Err = CursorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || CursorParseError(s.to_owned());
        let (txid, seq) = s.split_once('.').ok_or_else(error)?;
        let txid = txid.parse::<u64>().map_err(|_| error())?;
        let seq = seq.parse::<i64>().map_err(|_| error())?;
        if seq < 0 {
            return Err(error());
        }
        Ok(Self { txid, seq })
    }
}

impl Serialize for Cursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_its_string_form() {
        let cursor = Cursor::new(9_000_000_123, 42);
        assert_eq!(cursor.to_string().parse::<Cursor>().unwrap(), cursor);
        let json = serde_json::to_string(&cursor).unwrap();
        assert_eq!(json, "\"9000000123.42\"");
        assert_eq!(serde_json::from_str::<Cursor>(&json).unwrap(), cursor);
    }

    #[test]
    fn orders_by_txid_then_seq() {
        assert!(Cursor::new(5, 100) < Cursor::new(6, 1));
        assert!(Cursor::new(6, 1) < Cursor::new(6, 2));
        assert!(Cursor::ZERO < Cursor::new(0, 1));
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in ["", "1", "a.b", "1.-2", "1.2.3"] {
            assert!(bad.parse::<Cursor>().is_err(), "{bad} should not parse");
        }
    }
}
